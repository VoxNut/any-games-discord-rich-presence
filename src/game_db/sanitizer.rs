/// Converts raw binary names like "monster_hunter_world" or "eldenring" into readable titles
pub fn sanitize_to_display_title(raw_name: &str) -> String {
    let lower = raw_name.to_lowercase();
    let without_ext = lower.trim_end_matches(".exe");

    // Replace underscores, hyphens, and dots with spaces
    let with_spaces = without_ext
        .replace('_', " ")
        .replace('-', " ")
        .replace('.', " ");

    // Handle CamelCase / PascalCase or concatenated words (e.g. "eldenring" -> "Elden Ring" or capitalization)
    let words: Vec<&str> = with_spaces.split_whitespace().collect();
    if words.is_empty() {
        return raw_name.to_string();
    }

    let capitalized: Vec<String> = words
        .into_iter()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect();

    capitalized.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitizer() {
        assert_eq!(sanitize_to_display_title("elden_ring.exe"), "Elden Ring");
        assert_eq!(sanitize_to_display_title("hollow-knight.exe"), "Hollow Knight");
        assert_eq!(sanitize_to_display_title("witcher3"), "Witcher3");
        assert_eq!(sanitize_to_display_title("cyberpunk2077.exe"), "Cyberpunk2077");
    }
}
