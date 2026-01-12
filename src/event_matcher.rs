use crate::event::Event;
use chrono::{DateTime, Utc, FixedOffset, TimeZone};
use regex::Regex;
use std::collections::HashSet;

/// Confidence score for event matches
#[derive(Debug, Clone)]
pub struct MatchConfidence {
    pub text_similarity: f64,
    pub date_match: bool,
    pub category_match: bool,
    pub keyword_overlap: f64,
    pub number_match: bool,
    pub overall_score: f64,
}

impl MatchConfidence {
    pub fn is_high_confidence(&self) -> bool {
        self.overall_score >= 0.75
    }
    
    pub fn is_medium_confidence(&self) -> bool {
        self.overall_score >= 0.50 && self.overall_score < 0.75
    }
}

pub struct EventMatcher {
    similarity_threshold: f64,
}

impl EventMatcher {
    pub fn new(similarity_threshold: f64) -> Self {
        Self {
            similarity_threshold,
        }
    }

    pub fn normalize_text(&self, text: &str) -> String {
        text.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn extract_keywords(&self, text: &str) -> HashSet<String> {
        let stop_words: HashSet<&str> = [
            "will", "be", "the", "a", "an", "and", "or", "but", "in", "on",
            "at", "to", "for", "of", "with", "by",
        ]
        .iter()
        .cloned()
        .collect();

        self.normalize_text(text)
            .split_whitespace()
            .filter(|w| w.len() > 2 && !stop_words.contains(w))
            .map(|s| s.to_string())
            .collect()
    }

    pub fn extract_dates(&self, text: &str) -> Vec<String> {
        let patterns = [
            r"\b\d{1,2}[/-]\d{1,2}[/-]\d{2,4}\b",
            r"\b(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\s+\d{1,2},?\s+\d{4}\b",
            r"\b\d{4}\b",
            r"\b\d{4}-\d{2}-\d{2}\b", // ISO format
            r"\b\d{1,2}\s+(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\s+\d{4}\b",
        ];

        let mut dates = Vec::new();
        for pattern in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                for cap in re.captures_iter(text) {
                    dates.push(cap[0].to_string());
                }
            }
        }
        dates
    }

    /// Parse resolution date with multiple format support
    pub fn parse_resolution_date(&self, date_str: &str) -> Option<DateTime<Utc>> {
        // Try multiple date formats
        let formats = [
            "%Y-%m-%dT%H:%M:%S%.fZ",
            "%Y-%m-%dT%H:%M:%SZ",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%d",
            "%m/%d/%Y",
            "%d/%m/%Y",
            "%B %d, %Y",
            "%b %d, %Y",
        ];

        for format in &formats {
            if let Ok(dt) = DateTime::parse_from_str(date_str, format) {
                return Some(dt.with_timezone(&Utc));
            }
        }

        // Try RFC3339
        if let Ok(dt) = DateTime::parse_from_rfc3339(date_str) {
            return Some(dt.with_timezone(&Utc));
        }

        None
    }

    /// Check if two dates are within acceptable range (same day or within 24 hours)
    pub fn dates_match(&self, date1: Option<DateTime<Utc>>, date2: Option<DateTime<Utc>>) -> bool {
        match (date1, date2) {
            (Some(d1), Some(d2)) => {
                let diff = (d1 - d2).num_seconds().abs();
                diff <= 86400 // Within 24 hours
            }
            _ => false,
        }
    }

    pub fn extract_numbers(&self, text: &str) -> Vec<String> {
        let patterns = [
            r"\$[\d,]+(?:\.\d+)?",
            r"\d+%",
            r"\b\d{1,3}(?:,\d{3})*(?:\.\d+)?\b",
        ];

        let mut numbers = Vec::new();
        for pattern in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                for cap in re.captures_iter(text) {
                    numbers.push(cap[0].to_string());
                }
            }
        }
        numbers
    }

    pub fn calculate_similarity(&self, event1: &Event, event2: &Event) -> f64 {
        self.calculate_similarity_with_confidence(event1, event2).overall_score
    }

    pub fn calculate_similarity_with_confidence(&self, event1: &Event, event2: &Event) -> MatchConfidence {
        // Text similarity using strsim
        let title1 = self.normalize_text(&event1.title);
        let title2 = self.normalize_text(&event2.title);
        let text_similarity = strsim::jaro_winkler(&title1, &title2);

        // Keyword overlap
        let keywords1 = self.extract_keywords(&event1.title);
        let keywords2 = self.extract_keywords(&event2.title);

        let keyword_overlap = if !keywords1.is_empty() && !keywords2.is_empty() {
            let intersection: HashSet<_> = keywords1.intersection(&keywords2).collect();
            let union: HashSet<_> = keywords1.union(&keywords2).collect();
            intersection.len() as f64 / union.len() as f64
        } else {
            0.0
        };

        // Date matching - improved with resolution date comparison
        let date_match = self.dates_match(event1.resolution_date, event2.resolution_date);
        
        // Also check extracted dates from text
        let dates1 = self.extract_dates(&(event1.title.clone() + " " + &event1.description));
        let dates2 = self.extract_dates(&(event2.title.clone() + " " + &event2.description));
        let date_text_match = if !dates1.is_empty() && !dates2.is_empty() {
            let set1: HashSet<_> = dates1.iter().collect();
            let set2: HashSet<_> = dates2.iter().collect();
            !set1.is_disjoint(&set2) // Check if any dates overlap
        } else {
            false
        };
        
        let date_match_final = date_match || date_text_match;

        // Category matching
        let category_match = match (&event1.category, &event2.category) {
            (Some(c1), Some(c2)) => c1.to_lowercase() == c2.to_lowercase(),
            _ => false,
        };

        // Number matching
        let numbers1 = self.extract_numbers(&event1.title);
        let numbers2 = self.extract_numbers(&event2.title);
        let number_match = if !numbers1.is_empty() && !numbers2.is_empty() {
            let set1: HashSet<_> = numbers1.iter().collect();
            let set2: HashSet<_> = numbers2.iter().collect();
            !set1.is_disjoint(&set2) // Check if any numbers overlap
        } else {
            false
        };

        // Weighted combination
        let overall_score = text_similarity * 0.4
            + keyword_overlap * 0.25
            + if date_match_final { 0.15 } else { 0.0 }
            + if category_match { 0.1 } else { 0.0 }
            + if number_match { 0.1 } else { 0.0 };

        MatchConfidence {
            text_similarity,
            date_match: date_match_final,
            category_match,
            keyword_overlap,
            number_match,
            overall_score,
        }
    }

    pub fn find_matches(
        &self,
        polymarket_events: &[Event],
        kalshi_events: &[Event],
    ) -> Vec<(Event, Event, f64)> {
        self.find_matches_with_confidence(polymarket_events, kalshi_events)
            .into_iter()
            .map(|(e1, e2, conf)| (e1, e2, conf.overall_score))
            .collect()
    }

    pub fn find_matches_with_confidence(
        &self,
        polymarket_events: &[Event],
        kalshi_events: &[Event],
    ) -> Vec<(Event, Event, MatchConfidence)> {
        let mut matches = Vec::new();

        for pm_event in polymarket_events {
            for kalshi_event in kalshi_events {
                let confidence = self.calculate_similarity_with_confidence(pm_event, kalshi_event);

                if confidence.overall_score >= self.similarity_threshold {
                    matches.push((
                        pm_event.clone(),
                        kalshi_event.clone(),
                        confidence,
                    ));
                }
            }
        }

        // Sort by overall score (highest first)
        matches.sort_by(|a, b| {
            b.2.overall_score.partial_cmp(&a.2.overall_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        matches
    }

    pub fn find_best_match(
        &self,
        target_event: &Event,
        candidate_events: &[Event],
    ) -> Option<(Event, f64)> {
        let mut best_match: Option<(Event, f64)> = None;
        let mut best_similarity = 0.0;

        for candidate in candidate_events {
            let similarity = self.calculate_similarity(target_event, candidate);
            if similarity > best_similarity {
                best_similarity = similarity;
                best_match = Some((candidate.clone(), similarity));
            }
        }

        if best_similarity >= self.similarity_threshold {
            best_match
        } else {
            None
        }
    }
}

