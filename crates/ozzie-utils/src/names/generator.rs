use rand::Rng;

/// Adjectives themed around space, exploration, and science.
const LEFT: &[&str] = &[
    "cosmic", "stellar", "quantum", "nebular", "orbital",
    "galactic", "astral", "lunar", "solar", "plasma",
    "photon", "quasar", "pulsar", "nova", "hyper",
    "warp", "void", "dark", "bright", "frozen",
    "silent", "drifting", "distant", "ancient", "radiant",
    "chrome", "crimson", "golden", "silver", "azure",
    "swift", "bold", "fierce", "calm", "vivid",
    "epic", "prime", "deep", "keen", "vast",
    "rapid", "subtle", "lucid", "cryptic", "arcane",
    "primal", "spectral", "temporal", "parallel", "inverse",
];

/// SF author surnames and iconic character names.
const RIGHT: &[&str] = &[
    // Authors
    "asimov", "clarke", "hamilton", "herbert", "leguin",
    "dick", "bradbury", "verne", "wells", "heinlein",
    "gibson", "butler", "lem", "banks", "simmons",
    "haldeman", "bester", "zelazny", "wolfe", "stephenson",
    "jemisin", "leckie", "orwell", "huxley", "atwood",
    "ballard", "bear", "niven", "cherryh", "ellison",
    "sturgeon", "tiptree", "vance", "aldiss", "pohl",
    "silverberg", "robinson", "farmer", "delaney", "liu",
    "tchaikovsky", "corey", "scalzi", "reynolds", "baxter",
    "moorcock", "brunner",
    // Characters — Commonwealth Saga (Peter F. Hamilton)
    "ozzie", "sheldon", "myo", "kime", "burnelli",
    "kazimir", "mellanie", "morton", "bose", "edeard",
    "slvasta", "qatux", "tochee", "johansson", "halgarth",
    "justine", "gore", "nigel", "paula", "stig",
    // Characters — Dune (Frank Herbert)
    "leto", "chani", "jessica", "stilgar", "thufir",
    "gurney", "feyd", "rabban", "yueh", "irulan",
    "shaddam", "odrade", "taraza", "scytale", "teg",
    "hayt", "kynes", "jamis", "bijaz", "korba",
    // Characters — Hyperion Cantos (Dan Simmons)
    "kassad", "lamia", "silenus", "hoyt", "weintraub",
    "gladstone", "aenea", "nemes", "ummon", "albedo",
    "brawne", "lenar", "fedmahn", "meina", "arundez",
    "dure", "moneta", "siri",
    // Characters — Foundation + Robots (Asimov)
    "salvor", "bayta", "toran", "ebling", "arcadia",
    "magnifico", "channis", "demerzel", "baley", "fastolfe",
    "gladia", "vasilia", "amadiro", "trevize", "pelorat",
    "fallom", "compor", "branno", "palver", "novi", "jander",
    // Characters — Hitchhiker's Guide (Douglas Adams)
    "zaphod", "trillian", "marvin", "fenchurch", "zarniwoop",
    "agrajag", "wowbagger", "hotblack", "prosser", "hactar",
    "jeltz", "desiato", "prefect", "tricia", "prak",
    // Characters — Culture (Iain M. Banks)
    "gurgeh", "zakalwe", "horza", "quilan", "diziet",
    "djan", "linter", "kraiklyn", "balveda", "aviger",
    "oramen", "ferbin", "hippinse", "vateuil", "ziller", "genar",
    // Characters — Discworld (Terry Pratchett)
    "vetinari", "vimes", "angua", "ridcully", "tiffany",
    "rincewind", "weatherwax", "ogg", "nobby", "detritus",
    "dorfl", "dibbler", "stibbons", "conina", "teatime",
    "lipwig", "nitt",
    // Characters — The Expanse (James S.A. Corey)
    "holden", "naomi", "amos", "bobbie", "avasarala",
    "drummer", "ashford", "dawes", "prax", "clarissa",
    "peaches", "klaes", "camina", "filip", "marco", "duarte",
    // Characters — misc
    "hari", "daneel", "gaal", "muaddib", "alia",
    "deckard", "case", "genly", "shevek", "wintermute",
    "montag", "nemo", "elijah", "hal", "hiro",
    "molly", "breq", "essun", "ender", "valentine",
    "ripley", "solaris", "pris", "neuromancer", "dawn",
    "lilith", "seivarden",
];

/// Generates a random friendly name in the format "adjective_noun".
pub fn generate() -> String {
    let mut rng = rand::rng();
    let l = LEFT[rng.random_range(0..LEFT.len())];
    let r = RIGHT[rng.random_range(0..RIGHT.len())];
    format!("{l}_{r}")
}

/// Returns a name that does not collide according to the `exists` function.
///
/// Tries the base name, then appends `_0002`, `_0003` ... `_9999`.
/// If all collide, generates a new base name (up to 10 rounds).
/// Ultimate fallback: `"name_"` + 6 random hex chars.
pub fn generate_unique(mut exists: impl FnMut(&str) -> bool) -> String {
    for _ in 0..10 {
        let base = generate();
        if !exists(&base) {
            return base;
        }
        for suffix in 2..=9999 {
            let candidate = format!("{base}_{suffix:04}");
            if !exists(&candidate) {
                return candidate;
            }
        }
    }
    // Fallback: random hex
    let mut rng = rand::rng();
    let hex: u32 = rng.random_range(0..0x1000000);
    format!("name_{hex:06x}")
}

/// Creates a unique ID with the given prefix.
///
/// Format: `"{prefix}_{adjective}_{noun}"` or `"{prefix}_{adjective}_{noun}_{XXXX}"`.
pub fn generate_id(prefix: &str, mut exists: impl FnMut(&str) -> bool) -> String {
    let name = generate_unique(|n| exists(&format!("{prefix}_{n}")));
    format!("{prefix}_{name}")
}

/// Extracts the human-readable name from a prefixed ID.
///
/// `"sess_cosmic_asimov"` -> `"cosmic asimov"`
/// `"task_stellar_deckard_0002"` -> `"stellar deckard 0002"`
pub fn display_name(id: &str) -> &str {
    // Can't return owned String with underscores replaced easily,
    // so we return the slice after the first underscore.
    // The caller can replace underscores if needed for display.
    match id.find('_') {
        Some(i) => &id[i + 1..],
        None => id,
    }
}

/// Extracts the human-readable name, replacing underscores with spaces.
pub fn display_name_pretty(id: &str) -> String {
    match id.find('_') {
        Some(i) => id[i + 1..].replace('_', " "),
        None => id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_format() {
        for _ in 0..100 {
            let name = generate();
            let parts: Vec<&str> = name.splitn(2, '_').collect();
            assert_eq!(parts.len(), 2, "expected adj_noun, got {name:?}");
            assert!(!parts[0].is_empty());
            assert!(!parts[1].is_empty());
        }
    }

    #[test]
    fn no_duplicates_in_lists() {
        let mut seen = std::collections::HashSet::new();
        for w in LEFT {
            assert!(seen.insert(w), "duplicate in LEFT: {w}");
        }
        seen.clear();
        for w in RIGHT {
            assert!(seen.insert(w), "duplicate in RIGHT: {w}");
        }
    }

    #[test]
    fn generate_distribution() {
        let mut names = std::collections::HashSet::new();
        for _ in 0..200 {
            names.insert(generate());
        }
        assert!(
            names.len() >= 100,
            "poor distribution: only {} unique in 200 draws",
            names.len()
        );
    }

    #[test]
    fn generate_unique_no_collision() {
        let name = generate_unique(|_| false);
        assert!(!name.is_empty());
        let parts: Vec<&str> = name.splitn(2, '_').collect();
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn generate_unique_with_collision() {
        let mut calls = 0;
        let name = generate_unique(|_| {
            calls += 1;
            calls == 1 // only first attempt collides
        });
        assert!(!name.is_empty());
        assert!(name.ends_with("_0002"), "expected _0002 suffix, got {name:?}");
    }

    #[test]
    fn generate_unique_fallback() {
        let name = generate_unique(|_| true);
        assert!(name.starts_with("name_"), "expected hex fallback, got {name:?}");
        assert_eq!(name.len(), 11); // "name_" + 6 hex chars
    }

    #[test]
    fn generate_id_format() {
        let id = generate_id("sess", |_| false);
        assert!(id.starts_with("sess_"), "got {id:?}");
        let parts: Vec<&str> = id.splitn(3, '_').collect();
        assert!(parts.len() >= 3, "expected prefix_adj_noun, got {id:?}");
    }

    #[test]
    fn generate_id_uniqueness() {
        let mut existing = std::collections::HashSet::new();
        let id1 = generate_id("task", |c| existing.contains(c));
        existing.insert(id1.clone());

        let id2 = generate_id("task", |c| existing.contains(c));
        assert_ne!(id1, id2);
    }

    #[test]
    fn display_name_extraction() {
        assert_eq!(display_name_pretty("sess_cosmic_asimov"), "cosmic asimov");
        assert_eq!(
            display_name_pretty("task_stellar_deckard_0002"),
            "stellar deckard 0002"
        );
        assert_eq!(display_name_pretty("mem_void_herbert"), "void herbert");
        assert_eq!(display_name_pretty("noprefix"), "noprefix");
    }

    #[test]
    fn list_sizes() {
        assert!(LEFT.len() >= 50, "expected >=50 adjectives, got {}", LEFT.len());
        assert!(
            RIGHT.len() >= 150,
            "expected >=150 nouns, got {}",
            RIGHT.len()
        );
    }
}
