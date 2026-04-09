//! Enrichment Pipeline for LLM Generation
//!
//! This module provides automatic prompt enrichment and output cleaning
//! to ensure high-quality, literary text generation.

use regex::Regex;
use std::collections::HashSet;

// ============================================================================
// Enrichment Constants
// ============================================================================

/// Density Instruction - Show, Don't Tell
const DENSITY_INSTRUCTION: &str = r#"
[WRITING DIRECTIVE]
Develop each action with precise sensory details:
- SIGHT: colors, lights, movements, facial expressions
- SOUND: noises, silences, tone of voice, rhythm
- TOUCH: textures, temperature, physical sensations
- SMELL: scents, ambient odors

GOLDEN RULE: ALWAYS replace emotion adjectives with physical manifestations.
- Instead of "he was frightened", describe: "his hands trembled", "his heart raced"
- Instead of "she was sad", describe: "her eyes filled with tears", "her voice broke"
"#;

/// Structure Constraint - Anti-clichés
const STRUCTURE_CONSTRAINT: &str = r#"
[LITERARY CONSTRAINTS]
ABSOLUTE PROHIBITIONS (AI clichés):
- Forbidden words: suddenly, abruptly, incredible, fantastic, extraordinary
- Forbidden words: a shiver ran through, without warning, at that precise moment
- Forbidden words: heart pounding, breath caught, eyes widened

RHYTHM AND VARIATION:
- Alternate between short sentences (impact) and long sentences (description)
- Avoid more than 3 sentences of the same length in a row
- Use sentence fragments for moments of intense action
"#;

/// Structure for multi-phrase modes (Story)
const MULTI_PHRASE_STRUCTURE: &str = r#"
[NARRATIVE STRUCTURE]
Compose your text in multiple sentences with:
1. An atmosphere/setup sentence
2. The main action or dialogue enriched
3. A reflection or internal reaction sentence
"#;

/// Structure for single phrase generation
const SINGLE_PHRASE_STRUCTURE: &str = r#"
[OUTPUT FORMAT]
Generate EXACTLY ONE SINGLE rich and evocative sentence.
This sentence must be complete, with appropriate final punctuation.
"#;

/// List of clichés to filter in post-processing
const CLICHE_PATTERNS: &[&str] = &[
    "suddenly,",
    "abruptly,",
    "without warning,",
    "at that precise moment,",
    "a shiver ran through",
    "heart pounding",
    "breath caught",
    "eyes widened",
    "all of a sudden,",
    "before anyone knew it,",
    "it was then that",
    "it goes without saying",
    "it must be said that",
    "it should not be forgotten that",
];

/// Output language instruction - ensures English output
const OUTPUT_LANGUAGE_INSTRUCTION: &str = r#"
[OUTPUT LANGUAGE]
You may receive prompts in various languages (French, Spanish, German, etc.).
ALWAYS understand and process the input, but generate your response in ENGLISH.
Your output must be in English regardless of the input language.
"#;

/// Common repetitive words to monitor
const REPETITIVE_WORDS: &[&str] = &[
    "then", "so", "next", "after", "as", "thus",
    "however", "nevertheless", "yet", "therefore", "for",
];

// ============================================================================
// Prompt Wrapping
// ============================================================================

/// Generation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationMode {
    /// Story mode - allows multiple phrases (NexaBrain Story)
    Story,
    /// Dialogue mode - one phrase per line
    Dialogue,
    /// Description mode - one rich phrase
    Description,
    /// Action mode - one impact phrase
    Action,
    /// Continue mode - one continuation phrase
    Continue,
    /// Free mode - one creative phrase
    Free,
    /// Other mode - one default phrase
    Other,
}

impl GenerationMode {
    pub fn from_string(mode: &str) -> Self {
        match mode.to_lowercase().as_str() {
            "story" | "scene" => GenerationMode::Story,
            "dialogue" => GenerationMode::Dialogue,
            "describe" | "description" => GenerationMode::Description,
            "action" => GenerationMode::Action,
            "continue" | "continuation" => GenerationMode::Continue,
            "free" | "creative" => GenerationMode::Free,
            _ => GenerationMode::Other,
        }
    }

    /// Determines if this mode allows multiple phrases
    pub fn allows_multiple_phrases(&self) -> bool {
        matches!(self, GenerationMode::Story)
    }
}

/// Enrichment configuration
#[derive(Debug, Clone)]
pub struct EnrichmentConfig {
    /// Generation mode
    pub mode: GenerationMode,
    /// Start phrase (for NexaBrain Story)
    pub start_phrase: Option<String>,
    /// End phrase (for NexaBrain Story)
    pub end_phrase: Option<String>,
    /// Content language
    pub language: String,
    /// Custom style
    pub custom_style: Option<String>,
}

impl Default for EnrichmentConfig {
    fn default() -> Self {
        Self {
            mode: GenerationMode::Other,
            start_phrase: None,
            end_phrase: None,
            language: "en".to_string(),
            custom_style: None,
        }
    }
}

/// Wraps the user prompt with enrichment instructions
pub fn wrap_enriched_prompt(user_prompt: &str, config: &EnrichmentConfig) -> String {
    let mut enriched = String::new();

    // System header
    enriched.push_str("[SYSTEM - GENERATION INSTRUCTIONS]\n\n");

    // Density instructions (always present)
    enriched.push_str(DENSITY_INSTRUCTION);
    enriched.push_str("\n\n");

    // Structure constraints (always present)
    enriched.push_str(STRUCTURE_CONSTRAINT);
    enriched.push_str("\n\n");

    // Output structure based on mode
    let allows_multiple = config.mode.allows_multiple_phrases();

    if allows_multiple {
        // Check if we have start/end phrases
        let has_start = config.start_phrase.is_some();
        let has_end = config.end_phrase.is_some();

        if has_start || has_end {
            enriched.push_str("[CUSTOM STRUCTURE]\n");

            if has_start {
                enriched.push_str(&format!(
                    "Start your text AFTER this phrase: \"{}\"\n",
                    config.start_phrase.as_ref().unwrap()
                ));
            }

            if has_end {
                enriched.push_str(&format!(
                    "End your text BEFORE this phrase: \"{}\"\n",
                    config.end_phrase.as_ref().unwrap()
                ));
            }

            enriched.push_str("Generate the content between these two phrases with an appropriate word count to create a smooth and natural text.\n\n");
        } else {
            // Story mode without start/end phrases - multiple phrases
            enriched.push_str(MULTI_PHRASE_STRUCTURE);
            enriched.push_str("\n\n");
        }
    } else {
        // Standard mode - single phrase
        enriched.push_str(SINGLE_PHRASE_STRUCTURE);
        enriched.push_str("\n\n");
    }

    // Custom style if present
    if let Some(ref style) = config.custom_style {
        enriched.push_str(&format!("[CUSTOM STYLE]\n{}\n\n", style));
    }

    // Output language - always ensure English output
    enriched.push_str(OUTPUT_LANGUAGE_INSTRUCTION);
    enriched.push_str("\n");

    // Target language (if different from English)
    if config.language != "en" {
        enriched.push_str(&format!("[TARGET LANGUAGE]\nGenerate the text in {}.\n\n",
            match config.language.as_str() {
                "fr" => "French",
                "es" => "Spanish",
                "de" => "German",
                "it" => "Italian",
                "pt" => "Portuguese",
                _ => &config.language
            }
        ));
    }

    // User prompt
    enriched.push_str("[USER PROMPT]\n");
    enriched.push_str(user_prompt);
    enriched.push_str("\n\n");

    // Final reminder for single phrase
    if !allows_multiple {
        enriched.push_str("[FINAL REMINDER]\n");
        enriched.push_str("⚠️ IMPORTANT: Generate EXACTLY ONE SINGLE SENTENCE. No more, no less.\n");
        enriched.push_str("The sentence must be literary, rich in sensory details, and end with a period.\n");
    }

    enriched
}

// ============================================================================
// Post-Processing
// ============================================================================

/// Cleans and improves the generated output
pub fn clean_output(text: &str, config: &EnrichmentConfig) -> String {
    let mut cleaned = text.to_string();

    // 1. Remove extra spaces
    let space_re = Regex::new(r"\s+").unwrap();
    cleaned = space_re.replace_all(&cleaned, " ").to_string();

    // 2. Remove clichés
    cleaned = remove_cliches(&cleaned);

    // 3. Normalize punctuation
    cleaned = normalize_punctuation(&cleaned);

    // 4. Detect and fix repetitions
    cleaned = fix_repetitions(&cleaned);

    // 5. For single phrase modes, ensure only one sentence
    if !config.mode.allows_multiple_phrases() {
        cleaned = ensure_single_phrase(&cleaned);
    }

    // 6. Final trim
    cleaned = cleaned.trim().to_string();

    // 7. Ensure it ends with a period
    if !cleaned.ends_with('.') && !cleaned.ends_with('!') && !cleaned.ends_with('?') && !cleaned.ends_with('…') {
        cleaned.push('.');
    }

    cleaned
}

/// Removes clichés from text
fn remove_cliches(text: &str) -> String {
    let mut result = text.to_string();

    for cliche in CLICHE_PATTERNS {
        // Replace the cliché with an alternative or remove it
        let cliche_lower = cliche.to_lowercase();
        if result.to_lowercase().contains(&cliche_lower) {
            // For clichés at the beginning of a sentence
            if result.to_lowercase().starts_with(&cliche_lower) {
                // Remove and capitalize the first letter
                let re = Regex::new(&format!(r"(?i)^{}\s*", regex::escape(cliche))).unwrap();
                result = re.replace(&result, "").to_string();
                if !result.is_empty() {
                    let mut chars: Vec<char> = result.chars().collect();
                    chars[0] = chars[0].to_uppercase().next().unwrap_or(chars[0]);
                    result = chars.into_iter().collect();
                }
            } else {
                // Remove the cliché elsewhere
                let re = Regex::new(&format!(r"(?i){}\s*", regex::escape(cliche))).unwrap();
                result = re.replace(&result, "").to_string();
            }
        }
    }

    result
}

/// Normalizes punctuation
fn normalize_punctuation(text: &str) -> String {
    let mut result = text.to_string();

    // Normalized ellipsis
    let ellipsis_re = Regex::new(r"\.{2,}").unwrap();
    result = ellipsis_re.replace_all(&result, "…").to_string();

    // Remove spaces before punctuation
    let space_before_punct = Regex::new(r"\s+([.,!?;:])").unwrap();
    result = space_before_punct.replace_all(&result, "$1").to_string();

    // Add spaces after punctuation if missing
    let space_after_punct = Regex::new(r"([.,!?;:])([A-Za-zÀ-ÖØ-öø-ÿ])").unwrap();
    result = space_after_punct.replace_all(&result, "$1 $2").to_string();

    // Smart quotes
    let quote_re = Regex::new(r#""([^"]+)""#).unwrap();
    result = quote_re.replace_all(&result, "\"$1\"").to_string();

    result
}

/// Fixes word repetitions
fn fix_repetitions(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 3 {
        return text.to_string();
    }

    let mut result = Vec::new();
    let mut recent_words = HashSet::new();
    let window_size = 5;

    for (i, &word) in words.iter().enumerate() {
        let word_lower = word.to_lowercase();

        // Check if the word is repetitive and in the window
        if REPETITIVE_WORDS.contains(&word_lower.as_str()) && recent_words.contains(&word_lower) {
            // Skip the repetitive word
            continue;
        }

        result.push(word);

        // Maintain the sliding window
        if i >= window_size {
            if let Some(old_word) = words.get(i - window_size) {
                recent_words.remove(&old_word.to_lowercase());
            }
        }
        recent_words.insert(word_lower);
    }

    result.join(" ")
}

/// Ensures the text contains only one sentence
fn ensure_single_phrase(text: &str) -> String {
    // Find the first complete sentence
    let sentence_enders = ['.', '!', '?', '…'];

    // Find the end of the first sentence
    let mut end_pos = text.len();
    let mut found_end = false;

    for (i, c) in text.char_indices() {
        if sentence_enders.contains(&c) {
            end_pos = i + c.len_utf8();
            found_end = true;
            break;
        }
    }

    if found_end {
        text[..end_pos].to_string()
    } else {
        // If no period, add one
        format!("{}.", text.trim())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_enriched_prompt_single_phrase() {
        let config = EnrichmentConfig {
            mode: GenerationMode::Continue,
            ..Default::default()
        };

        let result = wrap_enriched_prompt("Continue the story", &config);

        assert!(result.contains("EXACTELY ONE SINGLE SENTENCE") || result.contains("EXACTLY ONE SINGLE SENTENCE"));
    }

    #[test]
    fn test_wrap_enriched_prompt_story_mode() {
        let config = EnrichmentConfig {
            mode: GenerationMode::Story,
            start_phrase: Some("The sun was setting.".to_string()),
            end_phrase: Some("She smiled.".to_string()),
            ..Default::default()
        };

        let result = wrap_enriched_prompt("Describe the scene", &config);

        assert!(result.contains("CUSTOM STRUCTURE"));
        assert!(result.contains("The sun was setting"));
        assert!(result.contains("She smiled"));
    }

    #[test]
    fn test_clean_output_single_phrase() {
        let config = EnrichmentConfig {
            mode: GenerationMode::Continue,
            ..Default::default()
        };

        let input = "Night fell over the city. Lights came on one by one.";
        let result = clean_output(input, &config);

        // Should keep only the first sentence
        assert_eq!(result, "Night fell over the city.");
    }

    #[test]
    fn test_clean_output_story_mode() {
        let config = EnrichmentConfig {
            mode: GenerationMode::Story,
            ..Default::default()
        };

        let input = "Night fell. Lights shone. The city awakened.";
        let result = clean_output(input, &config);

        // Should keep all sentences
        assert!(result.contains("Night") && result.contains("Lights") && result.contains("city"));
    }

    #[test]
    fn test_remove_cliches() {
        let input = "Suddenly, she understood that everything had changed.";
        let result = remove_cliches(input);

        assert!(!result.to_lowercase().contains("suddenly"));
    }

    #[test]
    fn test_normalize_punctuation() {
        let input = "She said:\"Hello!\" ";
        let result = normalize_punctuation(input);

        assert!(result.contains("\""));
    }
}
