use std::sync::Arc;

pub fn get_suggestions(input: Arc<str>, options: &Vec<Arc<str>>) -> Vec<(usize, Arc<str>)> {
    let mut suggestions = Vec::new();

    for option in options {
        let score = levenshtein::levenshtein(input.as_ref(), option);
        suggestions.push((score, option.clone()));
    }
    suggestions.sort_by(|a, b| a.0.cmp(&b.0));
    suggestions
}
