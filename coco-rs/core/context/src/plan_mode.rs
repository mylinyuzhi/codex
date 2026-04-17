//! Plan mode file management: slug generation, CRUD, recovery.
//!
//! TS: utils/plans.ts, utils/words.ts
//!
//! Plans are stored as markdown files at `~/.cocode/plans/{slug}.md` where
//! the slug is a random `{adjective}-{verb}-{noun}` word combination.
//! Each session gets a unique slug cached for its lifetime.

use coco_types::ToolName;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;

/// Maximum retries when a generated slug collides with an existing file.
const MAX_SLUG_RETRIES: i32 = 10;

// ── Word lists (TS: utils/words.ts) ──

const ADJECTIVES: &[&str] = &[
    "abundant",
    "ancient",
    "bright",
    "calm",
    "cheerful",
    "clever",
    "cozy",
    "curious",
    "dapper",
    "dazzling",
    "deep",
    "delightful",
    "eager",
    "elegant",
    "enchanted",
    "fancy",
    "fluffy",
    "gentle",
    "gleaming",
    "golden",
    "graceful",
    "happy",
    "hidden",
    "humble",
    "jolly",
    "joyful",
    "keen",
    "kind",
    "lively",
    "lovely",
    "lucky",
    "luminous",
    "magical",
    "majestic",
    "mellow",
    "merry",
    "mighty",
    "misty",
    "noble",
    "peaceful",
    "playful",
    "polished",
    "precious",
    "proud",
    "quiet",
    "quirky",
    "radiant",
    "rosy",
    "serene",
    "shiny",
    "silly",
    "sleepy",
    "smooth",
    "snazzy",
    "snug",
    "soft",
    "sparkling",
    "spicy",
    "splendid",
    "sprightly",
    "starry",
    "steady",
    "sunny",
    "swift",
    "tender",
    "tidy",
    "toasty",
    "tranquil",
    "valiant",
    "vast",
    "velvet",
    "vivid",
    "warm",
    "whimsical",
    "wild",
    "wise",
    "witty",
    "wondrous",
    "zany",
    "zesty",
    "zippy",
    "breezy",
    "bubbly",
    "cosmic",
    "crispy",
    "dreamy",
    "ethereal",
    "fizzy",
    "fuzzy",
    "glimmering",
    "glowing",
    "groovy",
    "iridescent",
    "jazzy",
    "melodic",
    "moonlit",
    "mossy",
    "peppy",
    "shimmering",
    "squishy",
    "velvety",
    // Programming concepts
    "abstract",
    "agile",
    "async",
    "atomic",
    "cached",
    "compiled",
    "concurrent",
    "dynamic",
    "functional",
    "generic",
    "hashed",
    "idempotent",
    "immutable",
    "iterative",
    "lazy",
    "linked",
    "modular",
    "optimized",
    "parallel",
    "pure",
    "reactive",
    "recursive",
    "resilient",
    "robust",
    "scalable",
    "sorted",
    "stateless",
    "streamed",
    "structured",
    "typed",
    "unified",
    "validated",
];

const VERBS: &[&str] = &[
    "baking",
    "beaming",
    "bouncing",
    "brewing",
    "bubbling",
    "chasing",
    "conjuring",
    "crafting",
    "crunching",
    "dancing",
    "discovering",
    "doodling",
    "dreaming",
    "drifting",
    "enchanting",
    "exploring",
    "floating",
    "foraging",
    "forging",
    "frolicking",
    "gathering",
    "giggling",
    "gliding",
    "growing",
    "hatching",
    "hopping",
    "hugging",
    "humming",
    "imagining",
    "inventing",
    "jingling",
    "juggling",
    "jumping",
    "kindling",
    "knitting",
    "launching",
    "leaping",
    "mapping",
    "meandering",
    "mixing",
    "munching",
    "napping",
    "nibbling",
    "noodling",
    "orbiting",
    "painting",
    "pondering",
    "popping",
    "prancing",
    "purring",
    "puzzling",
    "questing",
    "roaming",
    "rolling",
    "scribbling",
    "seeking",
    "singing",
    "skipping",
    "snuggling",
    "soaring",
    "sparking",
    "spinning",
    "splashing",
    "sprouting",
    "stargazing",
    "stirring",
    "strolling",
    "swimming",
    "tickling",
    "tinkering",
    "toasting",
    "tumbling",
    "twirling",
    "waddling",
    "wandering",
    "weaving",
    "whistling",
    "wiggling",
    "wishing",
    "wondering",
    "zooming",
];

const NOUNS: &[&str] = &[
    // Nature & cosmic
    "aurora",
    "blossom",
    "breeze",
    "brook",
    "bubble",
    "canyon",
    "cascade",
    "cloud",
    "comet",
    "coral",
    "cosmos",
    "crescent",
    "crystal",
    "dawn",
    "dewdrop",
    "eclipse",
    "ember",
    "feather",
    "fern",
    "firefly",
    "flame",
    "forest",
    "frost",
    "galaxy",
    "garden",
    "glacier",
    "glade",
    "grove",
    "harbor",
    "horizon",
    "island",
    "lagoon",
    "lake",
    "leaf",
    "meadow",
    "meteor",
    "mist",
    "moon",
    "moonbeam",
    "mountain",
    "nebula",
    "nova",
    "ocean",
    "orbit",
    "petal",
    "pine",
    "planet",
    "pond",
    "quasar",
    "rain",
    "rainbow",
    "reef",
    "ripple",
    "river",
    "shore",
    "sky",
    "snowflake",
    "spark",
    "spring",
    "star",
    "stardust",
    "starlight",
    "storm",
    "stream",
    "summit",
    "sun",
    "sunrise",
    "sunset",
    "thunder",
    "tide",
    "twilight",
    "valley",
    "waterfall",
    "wave",
    "willow",
    "wind",
    // Cute creatures
    "alpaca",
    "axolotl",
    "badger",
    "bear",
    "beaver",
    "bee",
    "bunny",
    "cat",
    "chipmunk",
    "crane",
    "deer",
    "dolphin",
    "dragon",
    "dragonfly",
    "duckling",
    "eagle",
    "elephant",
    "falcon",
    "finch",
    "flamingo",
    "fox",
    "frog",
    "hedgehog",
    "hummingbird",
    "jellyfish",
    "kitten",
    "koala",
    "ladybug",
    "lemur",
    "llama",
    "lynx",
    "narwhal",
    "newt",
    "octopus",
    "otter",
    "owl",
    "panda",
    "parrot",
    "peacock",
    "penguin",
    "phoenix",
    "platypus",
    "puffin",
    "puppy",
    "quail",
    "quokka",
    "rabbit",
    "raccoon",
    "raven",
    "seahorse",
    "seal",
    "sloth",
    "snail",
    "sparrow",
    "squirrel",
    "starfish",
    "swan",
    "tiger",
    "toucan",
    "turtle",
    "unicorn",
    "whale",
    "wolf",
    "wombat",
    // Fun objects
    "acorn",
    "anchor",
    "balloon",
    "beacon",
    "biscuit",
    "cake",
    "candle",
    "castle",
    "clock",
    "cookie",
    "crayon",
    "crown",
    "cupcake",
    "donut",
    "dream",
    "flask",
    "flute",
    "fountain",
    "gem",
    "globe",
    "goblet",
    "hammock",
    "harp",
    "haven",
    "hearth",
    "honey",
    "journal",
    "kettle",
    "key",
    "kite",
    "lantern",
    "lighthouse",
    "locket",
    "melody",
    "mitten",
    "muffin",
    "nest",
    "noodle",
    "oasis",
    "origami",
    "pancake",
    "peach",
    "pearl",
    "pie",
    "pillow",
    "pixel",
    "popcorn",
    "pretzel",
    "prism",
    "pudding",
    "pumpkin",
    "puzzle",
    "quill",
    "quilt",
    "riddle",
    "rocket",
    "rose",
    "scroll",
    "shell",
    "sketch",
    "sonnet",
    "sparkle",
    "sprout",
    "sundae",
    "taco",
    "teacup",
    "teapot",
    "toast",
    "token",
    "tower",
    "treasure",
    "trinket",
    "truffle",
    "tulip",
    "umbrella",
    "waffle",
    "wand",
    "whisper",
    "widget",
    "zephyr",
];

// ── Slug generation ──

/// Generate a random word slug in the format `adjective-verb-noun`.
///
/// Example: "gleaming-brewing-phoenix", "cosmic-pondering-lighthouse"
pub fn generate_word_slug() -> String {
    use std::hash::BuildHasher;
    use std::hash::RandomState;

    let hasher = RandomState::new();
    let h = hasher.hash_one(std::time::Instant::now());

    let adj = ADJECTIVES[(h as usize) % ADJECTIVES.len()];
    let verb = VERBS[((h >> 16) as usize) % VERBS.len()];
    let noun = NOUNS[((h >> 32) as usize) % NOUNS.len()];

    format!("{adj}-{verb}-{noun}")
}

// ── Slug cache (per session) ──

fn slug_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get or generate a plan slug for the given session.
///
/// The slug is generated lazily on first access and cached for the session.
/// If a plan file with the generated slug already exists, retries up to 10 times.
pub fn get_plan_slug(session_id: &str, plans_dir: &Path) -> String {
    if let Ok(cache) = slug_cache().lock()
        && let Some(slug) = cache.get(session_id)
    {
        return slug.clone();
    }

    let mut slug = String::new();
    for _ in 0..MAX_SLUG_RETRIES {
        slug = generate_word_slug();
        let file_path = plans_dir.join(format!("{slug}.md"));
        if !file_path.exists() {
            break;
        }
    }

    if let Ok(mut cache) = slug_cache().lock() {
        cache.insert(session_id.to_string(), slug.clone());
    }

    slug
}

/// Set a specific plan slug for a session (used when resuming).
pub fn set_plan_slug(session_id: &str, slug: &str) {
    if let Ok(mut cache) = slug_cache().lock() {
        cache.insert(session_id.to_string(), slug.to_string());
    }
}

/// Clear the plan slug for a session (called on /clear).
pub fn clear_plan_slug(session_id: &str) {
    if let Ok(mut cache) = slug_cache().lock() {
        cache.remove(session_id);
    }
}

/// Clear all plan slug entries (all sessions).
pub fn clear_all_plan_slugs() {
    if let Ok(mut cache) = slug_cache().lock() {
        cache.clear();
    }
}

// ── Plans directory ──

/// Resolve the plans directory from settings or default.
///
/// Default: `~/.cocode/plans/`. If `plans_directory` setting is set,
/// resolves it relative to the project root and validates it stays within.
pub fn resolve_plans_directory(
    config_dir: &Path,
    project_dir: Option<&Path>,
    plans_directory_setting: Option<&str>,
) -> PathBuf {
    if let Some(setting) = plans_directory_setting
        && let Some(proj) = project_dir
    {
        let resolved = proj.join(setting);
        // Validate path stays within project root
        if let (Ok(canonical_proj), Ok(canonical_resolved)) =
            (proj.canonicalize(), resolved.canonicalize())
            && canonical_resolved.starts_with(&canonical_proj)
        {
            return canonical_resolved;
        }
        // Fall back if canonicalize fails (dir doesn't exist yet) but looks safe
        if !setting.contains("..") {
            return resolved;
        }
        tracing::warn!("plansDirectory must be within project root: {setting}, using default");
    }

    config_dir.join("plans")
}

// ── Plan file CRUD ──

/// Get the file path for a session's plan.
///
/// Main conversation: `{slug}.md`
/// Subagents: `{slug}-agent-{agent_id}.md`
pub fn get_plan_file_path(session_id: &str, plans_dir: &Path, agent_id: Option<&str>) -> PathBuf {
    let slug = get_plan_slug(session_id, plans_dir);
    match agent_id {
        None => plans_dir.join(format!("{slug}.md")),
        Some(id) => plans_dir.join(format!("{slug}-agent-{id}.md")),
    }
}

/// Read the plan content for a session. Returns `None` if no plan exists.
pub fn get_plan(session_id: &str, plans_dir: &Path, agent_id: Option<&str>) -> Option<String> {
    let file_path = get_plan_file_path(session_id, plans_dir, agent_id);
    std::fs::read_to_string(file_path).ok()
}

/// Write plan content for a session.
pub fn write_plan(
    session_id: &str,
    plans_dir: &Path,
    content: &str,
    agent_id: Option<&str>,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(plans_dir)?;
    let file_path = get_plan_file_path(session_id, plans_dir, agent_id);
    std::fs::write(&file_path, content)?;
    Ok(())
}

/// Delete a plan file for a session.
pub fn delete_plan(
    session_id: &str,
    plans_dir: &Path,
    agent_id: Option<&str>,
) -> anyhow::Result<()> {
    let file_path = get_plan_file_path(session_id, plans_dir, agent_id);
    if file_path.exists() {
        std::fs::remove_file(file_path)?;
    }
    Ok(())
}

/// Check if a plan file exists for a session.
pub fn plan_exists(session_id: &str, plans_dir: &Path, agent_id: Option<&str>) -> bool {
    get_plan_file_path(session_id, plans_dir, agent_id).exists()
}

// ── Plan recovery (for resume) ──

/// Recovery sources for plan content, tried in priority order:
/// 1. File snapshot from transcript
/// 2. ExitPlanMode tool_use input
/// 3. planContent field on user messages
/// 4. plan_file_reference attachment
///
/// TS: copyPlanForResume() in utils/plans.ts
pub fn recover_plan_for_resume(
    session_id: &str,
    plans_dir: &Path,
    slug: &str,
    transcript_entries: &[serde_json::Value],
) -> bool {
    // Set the slug for the resumed session
    set_plan_slug(session_id, slug);

    let plan_path = plans_dir.join(format!("{slug}.md"));

    // If file already exists, nothing to recover
    if plan_path.exists() {
        return true;
    }

    // Try to recover content from transcript entries (search backwards)
    if let Some(content) = recover_plan_from_messages(transcript_entries) {
        std::fs::create_dir_all(plans_dir).ok();
        if std::fs::write(&plan_path, content).is_ok() {
            return true;
        }
    }

    false
}

/// Recover plan content from message history.
///
/// Searches backwards through transcript entries for plan content in:
/// 1. ExitPlanMode tool_use input (plan field)
/// 2. planContent field on user messages
/// 3. plan_file_reference attachments
fn recover_plan_from_messages(entries: &[serde_json::Value]) -> Option<String> {
    for entry in entries.iter().rev() {
        // Check for ExitPlanMode tool_use input
        if entry.get("role").and_then(|v| v.as_str()) == Some("assistant")
            && let Some(content) = entry.get("content").and_then(|v| v.as_array())
        {
            for block in content {
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_use")
                    && block.get("name").and_then(|v| v.as_str())
                        == Some(ToolName::ExitPlanMode.as_str())
                    && let Some(plan) = block
                        .get("input")
                        .and_then(|v| v.get("plan"))
                        .and_then(|v| v.as_str())
                    && !plan.is_empty()
                {
                    return Some(plan.to_string());
                }
            }
        }

        // Check for planContent on user messages
        if entry.get("role").and_then(|v| v.as_str()) == Some("user")
            && let Some(plan) = entry.get("planContent").and_then(|v| v.as_str())
            && !plan.is_empty()
        {
            return Some(plan.to_string());
        }

        // Check for plan_file_reference attachment
        if let Some(attachment) = entry.get("attachment")
            && attachment.get("type").and_then(|v| v.as_str()) == Some("plan_file_reference")
            && let Some(plan) = attachment.get("planContent").and_then(|v| v.as_str())
            && !plan.is_empty()
        {
            return Some(plan.to_string());
        }
    }
    None
}

/// Copy a plan file for a forked session. Generates a NEW slug for the fork
/// to prevent the original and forked sessions from clobbering each other.
pub fn copy_plan_for_fork(
    source_session_id: &str,
    target_session_id: &str,
    plans_dir: &Path,
) -> bool {
    let source_path = get_plan_file_path(source_session_id, plans_dir, None);
    if !source_path.exists() {
        return false;
    }
    let target_path = get_plan_file_path(target_session_id, plans_dir, None);
    std::fs::copy(source_path, target_path).is_ok()
}

#[cfg(test)]
#[path = "plan_mode.test.rs"]
mod tests;
