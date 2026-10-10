use super::{utf16_range_to_utf8, SpellCheckReceiver, SpellChecker, SpellError};
use block2::RcBlock;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSSpellChecker, NSTextCheckingOrthographyKey};
use objc2_foundation::{
    NSArray, NSDictionary, NSOrthography, NSRange, NSString, NSTextCheckingResult,
    NSTextCheckingType,
};
use std::ops::Range;
use std::sync::{Arc, Mutex};

pub(crate) struct MacSpellChecker {
    checker: objc2::rc::Retained<NSSpellChecker>,
    tag: isize,
}

impl MacSpellChecker {
    pub(crate) fn new() -> Self {
        let checker = NSSpellChecker::sharedSpellChecker();
        let tag = NSSpellChecker::uniqueSpellDocumentTag();
        Self { checker, tag }
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

        let selected = if languages.is_empty() {
            vec![None]
        } else {
            languages.iter().map(Some).collect()
        };
        let aggregate = Arc::new(Mutex::new(vec![None; selected.len()]));
        let projected = NSString::from_str(&text);
        let range = NSRange::new(0, text.encode_utf16().count());

        for (index, language) in selected.iter().enumerate() {
            let options = language.map(|language| {
                let language = NSString::from_str(language);
                let orthography = NSOrthography::defaultOrthographyForLanguage(&language);
                let key = unsafe { NSTextCheckingOrthographyKey };
                let values: [&AnyObject; 1] = [orthography.as_ref()];
                NSDictionary::from_slices(&[key], &values)
            });
            let results = aggregate.clone();
            let text = text.clone();
            let sender = sender.clone();
            let block = RcBlock::new(
                move |_sequence: isize,
                      result_array: std::ptr::NonNull<NSArray<NSTextCheckingResult>>,
                      _orthography: std::ptr::NonNull<NSOrthography>,
                      _word_count: isize| {
                    // SAFETY: The callback lends the result array for this call. Only primitive
                    // ranges and the captured Rust-owned source string cross thread boundaries.
                    let found = unsafe { result_array.as_ref() }
                        .iter()
                        .filter(|result| result.resultType() == NSTextCheckingType::Spelling)
                        .filter_map(|result| {
                            let range = result.range();
                            let end = range.location.checked_add(range.length)?;
                            utf16_range_to_utf8(&text, range.location..end)
                        })
                        .collect::<Vec<Range<usize>>>();
                    let Ok(mut aggregate) = results.lock() else {
                        return;
                    };
                    aggregate[index] = Some(found);
                    if aggregate.iter().all(Option::is_some) {
                        let Some(completed) = aggregate
                            .iter_mut()
                            .map(Option::take)
                            .collect::<Option<Vec<_>>>()
                        else {
                            return;
                        };
                        let _ = sender.try_send(Ok(completed));
                    }
                },
            );
            let checking_types = NSTextCheckingType::Spelling.bits();
            // SAFETY: NSSpellChecker retains the completion block for the asynchronous request.
            // The captured callback state is Rust-owned and does not access AppKit or GPUI.
            unsafe {
                self.checker
                    .requestCheckingOfString_range_types_options_inSpellDocumentWithTag_completionHandler(
                        &projected,
                        range,
                        checking_types,
                        options.as_deref(),
                        self.tag,
                        Some(&block),
                    );
            }
        }
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
        let native_word = NSString::from_str(word);
        let range = NSRange::new(0, word.encode_utf16().count());
        let selected = if languages.is_empty() {
            vec![None]
        } else {
            languages.iter().map(Some).collect()
        };
        let mut suggestions = Vec::new();
        for language in selected {
            let language = language.map(|language| NSString::from_str(language));
            if let Some(guesses) = self
                .checker
                .guessesForWordRange_inString_language_inSpellDocumentWithTag(
                    range,
                    &native_word,
                    language.as_deref(),
                    self.tag,
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
        self.checker
            .ignoreWord_inSpellDocumentWithTag(&NSString::from_str(word), self.tag);
    }

    fn learn(&self, word: &str) {
        self.checker.learnWord(&NSString::from_str(word));
    }
}

impl Drop for MacSpellChecker {
    fn drop(&mut self) {
        self.checker.closeSpellDocumentWithTag(self.tag);
    }
}
