use std::path::Path;

#[derive(Debug)]
pub enum FileReadError {
    UnsupportedExtension,
    Io(std::io::Error),
}

impl std::fmt::Display for FileReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedExtension => write!(f, "file extension is not permitted for reading"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl std::error::Error for FileReadError {}

pub fn read_allowed_file(path: impl AsRef<Path>) -> Result<Vec<u8>, FileReadError> {
    let path = path.as_ref();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match extension.as_deref() {
        Some("png") | Some("jpg") | Some("jpeg") | Some("webp") | Some("pdf") => {
            std::fs::read(path).map_err(FileReadError::Io)
        }
        _ => Err(FileReadError::UnsupportedExtension),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn accepts_allowed_file_extensions() {
        let path = std::env::temp_dir().join(format!("ai_bridge_eye_{}.png", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::write(&path, [137, 80, 78, 71]).unwrap();
        assert!(read_allowed_file(&path).is_ok());
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn rejects_forbidden_file_extensions() {
        let path = std::env::temp_dir().join(format!("ai_bridge_eye_{}.txt", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::write(&path, b"not allowed").unwrap();
        assert!(matches!(read_allowed_file(&path), Err(FileReadError::UnsupportedExtension)));
        fs::remove_file(&path).unwrap();
    }
}
