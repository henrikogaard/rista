use std::io::{self, Read};
use std::path::Path;

const MAX_TEXT_BYTES: u64 = 256 * 1024;

#[derive(Debug, PartialEq)]
pub enum Preview {
    Text(String),
    Binary,
    TooLarge,
}

pub fn extension(path: &Path) -> String {
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
}

pub fn is_document(path: &Path) -> bool {
    matches!(extension(path).as_str(), "md" | "markdown" | "base")
}

pub fn visible(path: &Path, show_other_files: bool) -> bool {
    show_other_files || is_document(path)
}

pub fn load(path: &Path) -> io::Result<Preview> {
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_TEXT_BYTES + 1).read_to_end(&mut bytes)?;
    Ok(decode(bytes))
}

fn decode(bytes: Vec<u8>) -> Preview {
    if bytes.len() > MAX_TEXT_BYTES as usize {
        return Preview::TooLarge;
    }
    match String::from_utf8(bytes) {
        Ok(text)
            if !text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) =>
        {
            Preview::Text(text)
        }
        _ => Preview::Binary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_preserves_documents_and_gates_attachments() {
        for name in ["note.md", "note.MARKDOWN", "data.base"] {
            assert!(visible(Path::new(name), false));
        }
        for name in ["photo.PNG", "report.pdf", "data.csv", "LICENSE"] {
            assert!(!visible(Path::new(name), false));
            assert!(visible(Path::new(name), true));
        }
        assert_eq!(extension(Path::new("photo.PNG")), "png");
    }

    #[test]
    fn previews_are_bounded_and_reject_binary_data() {
        assert_eq!(
            decode("hello æøå\n<script>literal</script>".as_bytes().to_vec()),
            Preview::Text("hello æøå\n<script>literal</script>".into())
        );
        assert_eq!(decode(vec![0, 1]), Preview::Binary);
        assert_eq!(decode(vec![255]), Preview::Binary);
        assert_eq!(
            decode(vec![b'a'; MAX_TEXT_BYTES as usize + 1]),
            Preview::TooLarge
        );
    }
}
