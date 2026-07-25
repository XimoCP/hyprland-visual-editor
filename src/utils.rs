use std::fs;
use std::path::Path;

/// Replaces content between two markers in a file.
/// If markers don't exist, creates the block at the end of the file.
/// Returns Ok(true) if content was replaced, Ok(false) if markers weren't found and block was appended.
#[allow(dead_code)]
pub fn edit_between_markers(
    path: &Path,
    start_marker: &str,
    end_marker: &str,
    new_content: &str,
) -> Result<bool, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;

    let new_block = format!("{}\n{}\n{}", start_marker, new_content.trim_end(), end_marker);

    let result = if let (Some(start_idx), Some(end_idx)) = (
        content.find(start_marker),
        content.find(end_marker),
    ) {
        let before = &content[..start_idx];
        let after = &content[end_idx + end_marker.len()..];
        format!("{}{}{}", before, new_block, after)
    } else {
        format!("{}\n{}\n", content.trim_end(), new_block)
    };

    fs::write(path, result)
        .map_err(|e| format!("Cannot write {}: {}", path.display(), e))?;

    Ok(content.find(start_marker).is_some())
}

/// Extracts content between two markers from a string.
/// Returns Ok(Some(content)) if found, Ok(None) if markers don't exist.
#[allow(dead_code)]
pub fn extract_block(content: &str, start_marker: &str, end_marker: &str) -> Option<String> {
    let start_idx = content.find(start_marker)?;
    let end_idx = content.find(end_marker)?;
    if end_idx <= start_idx {
        return None;
    }
    let block = &content[start_idx + start_marker.len()..end_idx];
    Some(block.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_extract_block_found() {
        let content = "line1\n//-- START\nblock content\n//-- END\nline2";
        let result = extract_block(content, "//-- START", "//-- END");
        assert_eq!(result, Some("block content".to_string()));
    }

    #[test]
    fn test_extract_block_not_found() {
        let content = "line1\nline2";
        let result = extract_block(content, "//-- START", "//-- END");
        assert_eq!(result, None);
    }

    #[test]
    fn test_edit_between_markers_replace() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.conf");
        fs::write(
            &path,
            "before\n//-- START\nold content\n//-- END\nafter",
        )
        .unwrap();

        let changed = edit_between_markers(&path, "//-- START", "//-- END", "new content").unwrap();
        assert!(changed);

        let result = fs::read_to_string(&path).unwrap();
        assert!(result.contains("new content"));
        assert!(!result.contains("old content"));
    }

    #[test]
    fn test_edit_between_markers_append() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.conf");
        fs::write(&path, "existing content").unwrap();

        let changed = edit_between_markers(&path, "//-- START", "//-- END", "new block").unwrap();
        assert!(!changed);

        let result = fs::read_to_string(&path).unwrap();
        assert!(result.contains("//-- START"));
        assert!(result.contains("new block"));
        assert!(result.contains("//-- END"));
    }
}
