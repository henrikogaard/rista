use super::{utf16_range_to_utf8, SpellCheckReceiver, SpellChecker, SpellError};
use block2::RcBlock;
use gpui_kit::ForegroundExecutor;
use objc2_app_kit::NSSpellChecker;
use objc2_foundation::{
    NSArray, NSOrthography, NSRange, NSString, NSTextCheckingResult, NSTextCheckingType,
};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

struct NativeDocument {
    checker: objc2::rc::Retained<NSSpellChecker>,
    tag: isize,
}

impl NativeDocument {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            checker: NSSpellChecker::sharedSpellChecker(),
            tag: NSSpellChecker::uniqueSpellDocumentTag(),
        })
    }
}

impl Drop for NativeDocument {
    fn drop(&mut self) {
        self.checker.closeSpellDocumentWithTag(self.tag);
    }
}

struct SpellQueue {
    gate: smol::lock::Mutex<()>,
    original_language: RefCell<Option<String>>,
}

impl SpellQueue {
    fn new() -> Self {
        Self {
            gate: smol::lock::Mutex::new(()),
            original_language: RefCell::new(None),
        }
    }
}

thread_local! {
    static SPELL_QUEUE: Rc<SpellQueue> = Rc::new(SpellQueue::new());
}

struct SpellPreferencesGuard {
    native: Rc<NativeDocument>,
    queue: Rc<SpellQueue>,
    original_language: String,
    original_automatic: bool,
}

impl SpellPreferencesGuard {
    fn new(native: Rc<NativeDocument>, queue: Rc<SpellQueue>) -> Self {
        let original_language = native.checker.language().to_string();
        let original_automatic = native.checker.automaticallyIdentifiesLanguages();
        queue
            .original_language
            .borrow_mut()
            .replace(original_language.clone());
        native.checker.setAutomaticallyIdentifiesLanguages(false);
        Self {
            native,
            queue,
            original_language,
            original_automatic,
        }
    }

    fn set_language(&self, language: &str) -> bool {
        self.native
            .checker
            .setLanguage(&NSString::from_str(language))
    }
}

impl Drop for SpellPreferencesGuard {
    fn drop(&mut self) {
        self.native
            .checker
            .setLanguage(&NSString::from_str(&self.original_language));
        self.native
            .checker
            .setAutomaticallyIdentifiesLanguages(self.original_automatic);
        self.queue.original_language.borrow_mut().take();
    }
}

pub(crate) struct MacSpellChecker {
    native: Rc<NativeDocument>,
    foreground_executor: ForegroundExecutor,
}

impl MacSpellChecker {
    pub(crate) fn new(foreground_executor: ForegroundExecutor) -> Self {
        Self {
            native: NativeDocument::new(),
            foreground_executor,
        }
    }
}

pub(crate) fn available_languages() -> Vec<String> {
    let checker = NSSpellChecker::sharedSpellChecker();
    checker
        .availableLanguages()
        .iter()
        .map(|language| language.to_string())
        .collect()
}

impl SpellChecker for MacSpellChecker {
    fn check(&self, text: String, languages: &[String]) -> SpellCheckReceiver {
        let (sender, receiver) = smol::channel::unbounded();
        let available = available_languages();
        let invalid = languages
            .iter()
            .filter(|language| !available.contains(*language))
            .cloned()
            .collect::<Vec<_>>();
        if !invalid.is_empty() {
            let _ = sender.try_send(Err(SpellError::InvalidLanguages(invalid)));
            return receiver;
        }

        let selected = languages.to_vec();
        let native = self.native.clone();
        let foreground_executor = self.foreground_executor.clone();
        let queue = SPELL_QUEUE.with(Rc::clone);
        foreground_executor
            .spawn(async move {
                let gate = queue.gate.lock().await;
                if sender.is_closed() {
                    return;
                }
                let result = run_check(native, text, selected, sender.clone(), queue.clone()).await;
                drop(gate);
                if !sender.is_closed() {
                    let _ = sender.try_send(result);
                }
            })
            .detach();
        receiver
    }

    fn suggestions(&self, word: &str, languages: &[String]) -> Vec<String> {
        let available = available_languages();
        if languages
            .iter()
            .any(|language| !available.contains(language))
        {
            return Vec::new();
        }
        let selected = if languages.is_empty() {
            vec![SPELL_QUEUE.with(|queue| queue.original_language.borrow().clone())]
        } else {
            languages.iter().cloned().map(Some).collect()
        };
        let native_word = NSString::from_str(word);
        let range = NSRange::new(0, word.encode_utf16().count());
        let mut suggestions = Vec::new();
        for language in selected {
            let language = language.map(|language| NSString::from_str(&language));
            if let Some(guesses) = self
                .native
                .checker
                .guessesForWordRange_inString_language_inSpellDocumentWithTag(
                    range,
                    &native_word,
                    language.as_deref(),
                    self.native.tag,
                )
            {
                for guess in guesses.iter() {
                    let guess = guess.to_string();
                    if !suggestions.contains(&guess) {
                        suggestions.push(guess);
                        if suggestions.len() == 8 {
                            return suggestions;
                        }
                    }
                }
            }
        }
        suggestions
    }

    fn ignore(&self, word: &str) {
        self.native
            .checker
            .ignoreWord_inSpellDocumentWithTag(&NSString::from_str(word), self.native.tag);
    }

    fn learn(&self, word: &str) {
        self.native.checker.learnWord(&NSString::from_str(word));
    }
}

async fn run_check(
    native: Rc<NativeDocument>,
    text: String,
    languages: Vec<String>,
    sender: smol::channel::Sender<Result<super::SpellResults, SpellError>>,
    queue: Rc<SpellQueue>,
) -> Result<super::SpellResults, SpellError> {
    let mut results = Vec::new();
    let preferences = if languages.is_empty() {
        None
    } else {
        Some(SpellPreferencesGuard::new(native.clone(), queue))
    };
    if languages.is_empty() {
        if sender.is_closed() {
            drop(preferences);
            return Ok(results);
        }
        if let Some(found) = request_check(native, &text).await {
            results.push(found);
        }
    } else {
        for language in languages {
            if sender.is_closed() {
                break;
            }
            let preferences = preferences.as_ref().expect("explicit languages");
            if !preferences.set_language(&language) {
                return Err(SpellError::InvalidLanguages(vec![language]));
            }
            if let Some(found) = request_check(native.clone(), &text).await {
                results.push(found);
            } else {
                break;
            }
        }
    }
    drop(preferences);
    Ok(results)
}

async fn request_check(native: Rc<NativeDocument>, text: &str) -> Option<Vec<Range<usize>>> {
    let (sender, receiver) = smol::channel::bounded(1);
    let projected = NSString::from_str(text);
    let range = NSRange::new(0, text.encode_utf16().count());
    let callback_text = text.to_owned();
    let block = RcBlock::new(
        move |_sequence: isize,
              result_array: std::ptr::NonNull<NSArray<NSTextCheckingResult>>,
              _orthography: std::ptr::NonNull<NSOrthography>,
              _word_count: isize| {
            let found = unsafe { result_array.as_ref() }
                .iter()
                .filter(|result| result.resultType() == NSTextCheckingType::Spelling)
                .filter_map(|result| {
                    let range = result.range();
                    let end = range.location.checked_add(range.length)?;
                    utf16_range_to_utf8(&callback_text, range.location..end)
                })
                .collect::<Vec<Range<usize>>>();
            let _ = sender.try_send(found);
        },
    );
    let checking_types = NSTextCheckingType::Spelling.bits();
    unsafe {
        native
            .checker
            .requestCheckingOfString_range_types_options_inSpellDocumentWithTag_completionHandler(
                &projected,
                range,
                checking_types,
                None,
                native.tag,
                Some(&block),
            );
    }
    receiver.recv().await.ok()
}
