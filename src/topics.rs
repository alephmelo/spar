//! Curated practice operations. Interests supply context, not the topic pool.
use crate::scheduler::{ExerciseShape, RecentRep};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Arrays,
    Strings,
    MapsAndSets,
    SearchingAndSorting,
    StacksAndQueues,
    Trees,
    Graphs,
    DynamicProgramming,
    NumbersAndBits,
    ApplicationLogic,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Topic {
    pub id: TopicId,
    pub category: Category,
    pub description: &'static str,
    pub skills: &'static [&'static str],
    pub min_minutes: u8,
}

// Define IDs and their metadata together so each ID has exactly one category.
macro_rules! catalog {
    ($( $id:ident, $category:ident, $minutes:literal, [$($skill:literal),+], $description:literal; )+) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "kebab-case")]
        pub enum TopicId { $( $id, )+ }

        pub const CATALOG: &[Topic] = &[$(Topic {
            id: TopicId::$id, category: Category::$category,
            description: $description, skills: &[$($skill),+], min_minutes: $minutes,
        }),+];

        impl TopicId {
            pub fn topic(self) -> &'static Topic {
                &CATALOG[self as usize]
            }
        }
    };
}

catalog! {
    Partitioning, Arrays, 1, ["boundaries", "collections"], "Partition a sequence into fixed-size pieces, retaining the final remainder. This is the batching subtopic.";
    SlidingWindow, Arrays, 5, ["boundaries", "collections"], "Compute a statistic over fixed-size contiguous windows, including the first and last valid window.";
    PrefixSums, Arrays, 5, ["boundaries", "transformations"], "Build cumulative totals or answer a small range-sum query with explicit endpoints.";
    TwoPointers, Arrays, 5, ["boundaries", "collections"], "Scan a sequence from two positions for one pairing or compaction rule; specify ordering and duplicates.";
    TextNormalization, Strings, 1, ["transformations", "error-handling"], "Normalize text with one precise casing, whitespace or separator policy; define the character domain.";
    TokenParsing, Strings, 1, ["boundaries", "transformations", "error-handling"], "Parse a short delimited token sequence with explicit empty-token and malformed-token behavior.";
    RunLengthEncoding, Strings, 1, ["boundaries", "transformations"], "Encode or decode consecutive runs of characters; handle the final run and empty text.";
    FrequencyCounting, MapsAndSets, 1, ["collections", "transformations"], "Count occurrences and return a specified deterministic representation.";
    Grouping, MapsAndSets, 1, ["collections", "transformations"], "Group records by a key while preserving required order; no chunk-size or capacity rule.";
    StableDeduplication, MapsAndSets, 1, ["collections", "boundaries"], "Remove duplicate keys with a precise first/last-wins policy and stable output order.";
    LookupJoin, MapsAndSets, 1, ["collections", "transformations", "error-handling"], "Join two small collections by key with an explicit policy for missing or duplicate matches.";
    BinarySearch, SearchingAndSorting, 5, ["boundaries", "collections"], "Find an index or insertion position in sorted values; specify absent targets and duplicate behavior.";
    StableRanking, SearchingAndSorting, 1, ["collections", "transformations"], "Rank a small collection by a primary key and explicit tie rule, optionally selecting the top k.";
    SortedMerge, SearchingAndSorting, 5, ["boundaries", "collections", "transformations"], "Merge two sorted sequences, preserving duplicates and handling an exhausted side.";
    DelimiterMatching, StacksAndQueues, 1, ["collections", "error-handling"], "Check nesting of a small set of delimiters, including mismatched and unmatched closers.";
    UndoOperations, StacksAndQueues, 1, ["collections", "transformations", "error-handling"], "Apply a sequence of edits and undo operations with a clear empty-stack policy.";
    QueueSimulation, StacksAndQueues, 1, ["boundaries", "collections", "error-handling"], "Simulate enqueue/dequeue operations with explicit empty-queue behavior; no batching or capacity threshold.";
    TreeLeaves, Trees, 5, ["collections", "transformations"], "Collect leaves in a tiny nested JSON tree in a specified traversal order; provide the traversal skeleton.";
    TreeDepth, Trees, 5, ["boundaries", "collections"], "Compute depth in a tiny nested JSON tree with explicit root/empty conventions and a traversal skeleton.";
    GraphReachability, Graphs, 5, ["collections", "error-handling"], "Traverse a tiny adjacency map from a start node with visited tracking; supply the traversal skeleton and missing-node policy.";
    CycleDetection, Graphs, 10, ["collections", "error-handling"], "Detect a directed cycle in a tiny dependency graph; supply traversal scaffolding so one decision remains.";
    RollingRecurrence, DynamicProgramming, 5, ["boundaries", "transformations"], "Complete a short recurrence over a sequence, with explicit base cases and an iteration skeleton.";
    GridPaths, DynamicProgramming, 10, ["boundaries", "collections"], "Complete one transition in a small grid-path calculation with provided table scaffolding.";
    MinimumCost, DynamicProgramming, 10, ["boundaries", "collections"], "Complete one transition for minimum cost along a short sequence with explicit base cases and scaffolding.";
    DigitOperations, NumbersAndBits, 1, ["boundaries", "transformations"], "Extract or transform integer digits with explicit zero and sign behavior.";
    IntegerDivision, NumbersAndBits, 1, ["boundaries", "error-handling"], "Implement a quotient/remainder or divisibility rule with explicit zero/sign semantics; no batching or capacity calculation.";
    BitFlags, NumbersAndBits, 1, ["boundaries", "transformations", "error-handling"], "Read, set or clear a small integer bit mask with a stated valid range and unknown-bit policy.";
    StateTransitions, ApplicationLogic, 1, ["collections", "error-handling"], "Apply explicit state transitions to a short event list, including invalid-transition behavior.";
    ConfigurationFallback, ApplicationLogic, 1, ["transformations", "error-handling"], "Resolve configuration precedence while distinguishing absent values from present false, zero or empty values.";
    RecordValidation, ApplicationLogic, 1, ["boundaries", "error-handling"], "Validate one record against a compact field contract; no byte-limit, batch-size or capacity problem.";
    IntervalOverlap, ApplicationLogic, 1, ["boundaries", "collections"], "Determine overlap or membership for explicitly open/closed intervals, including empty intervals.";
    RetryPolicy, ApplicationLogic, 1, ["boundaries", "error-handling"], "Apply a bounded retry or delay policy using only supplied attempt/error values; no clocks or network.";
    CacheExpiry, ApplicationLogic, 1, ["boundaries", "error-handling"], "Decide freshness from supplied timestamps and TTL with explicit expiry/zero-duration behavior.";
}

/// Three different categories, sampled without replacement. Recent topic choices
/// cool down for two reps; the latest category cools down when enough alternatives
/// remain. Older use reduces selection weight, without fixing catalog-order ties.
pub fn shortlist(skill: &str, minutes: u8, recent: &[RecentRep]) -> Vec<Topic> {
    let eligible: Vec<_> = CATALOG
        .iter()
        .filter(|topic| {
            topic.min_minutes <= minutes && (skill == "testing" || topic.skills.contains(&skill))
        })
        .collect();
    let cooled: Vec<_> = eligible
        .iter()
        .copied()
        .filter(|topic| {
            !recent
                .iter()
                .take(2)
                .any(|r| previous_topic(r) == Some(topic.id))
        })
        .collect();
    let categories = |pool: &[&Topic]| {
        let mut categories = Vec::new();
        for topic in pool {
            if !categories.contains(&topic.category) {
                categories.push(topic.category);
            }
        }
        categories
    };
    // Relax cooldown rather than fail generation if the catalog is narrowed later.
    let pool = if categories(&cooled).len() >= 3 {
        &cooled
    } else {
        &eligible
    };
    let mut choices = categories(pool);
    if choices.len() > 3
        && let Some(last) = recent.first().and_then(previous_topic)
    {
        choices.retain(|category| *category != last.topic().category);
    }
    let penalty = |matches: &dyn Fn(TopicId) -> bool| -> u32 {
        recent
            .iter()
            .take(12)
            .enumerate()
            .filter_map(|(index, rep)| {
                previous_topic(rep)
                    .filter(|id| matches(*id))
                    .map(|_| (12 - index) as u32)
            })
            .sum()
    };
    let mut ranked: Vec<_> = choices
        .into_iter()
        .map(|category| {
            (
                random_rank(penalty(&|id| id.topic().category == category)),
                category,
            )
        })
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    ranked
        .into_iter()
        .take(3)
        .map(|(_, category)| {
            **pool
                .iter()
                .filter(|topic| topic.category == category)
                .map(|topic| (random_rank(penalty(&|id| id == topic.id)), topic))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .expect("category has eligible topics")
                .1
        })
        .collect()
}

// Exponential race: independent random priorities sample with weight 1/(1+penalty).
// UUID v4 supplies fresh randomness without a new RNG dependency or persisted seed.
fn random_rank(penalty: u32) -> f64 {
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    let bits = u64::from_le_bytes(bytes[8..].try_into().unwrap()) >> 11;
    let uniform = (bits + 1) as f64 / ((1_u64 << 53) as f64 + 1.0);
    -uniform.ln() * (1.0 + f64::from(penalty))
}

// Legacy hints affect freshness only, never practice/proficiency. New packages
// store the actual selected ID; old shape metadata takes precedence over names.
fn previous_topic(rep: &RecentRep) -> Option<TopicId> {
    if rep.topic.is_some() {
        return rep.topic;
    }
    if let Some(design) = rep.design {
        let mapped = match design.shape {
            ExerciseShape::Chunking => Some(TopicId::Partitioning),
            ExerciseShape::Intervals => Some(TopicId::IntervalOverlap),
            ExerciseShape::Validation => Some(TopicId::RecordValidation),
            ExerciseShape::RetryLimits => Some(TopicId::RetryPolicy),
            ExerciseShape::Grouping => Some(TopicId::Grouping),
            ExerciseShape::StableDeduplication => Some(TopicId::StableDeduplication),
            ExerciseShape::TopSelection => Some(TopicId::StableRanking),
            ExerciseShape::Fallback => Some(TopicId::ConfigurationFallback),
            // These old shapes span several new topics; do not invent a match.
            ExerciseShape::Thresholds
            | ExerciseShape::Normalization
            | ExerciseShape::Projection => None,
        };
        if mapped.is_some() {
            return mapped;
        }
    }
    let family = rep.family.to_lowercase();
    if family.contains("batch") || family.contains("chunk") {
        Some(TopicId::Partitioning)
    } else if family.contains("cache") || family.contains("freshness") {
        Some(TopicId::CacheExpiry)
    } else if family.contains("overlap") || family.contains("interval") {
        Some(TopicId::IntervalOverlap)
    } else if family.contains("record") && family.contains("limit") {
        Some(TopicId::RecordValidation)
    } else {
        None
    }
}
