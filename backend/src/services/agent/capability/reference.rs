//! The grammar of capability ids, in one place.

const SKILL_PREFIX: &str = "skill:";
const MCP_PREFIX: &str = "mcp.";

/// What a capability id names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityRef<'a> {
    /// A capability registered by the platform, e.g. `phantasi.search`.
    Builtin(&'a str),
    /// `skill:{skill_id}`.
    Skill(&'a str),
    /// `mcp.{server_id}.{tool}`. Server ids may contain dots, so the pair is
    /// only known once matched against the advertised tools.
    Mcp(&'a str),
}

impl<'a> CapabilityRef<'a> {
    pub fn parse(id: &'a str) -> Self {
        if let Some(skill_id) = id.strip_prefix(SKILL_PREFIX) {
            Self::Skill(skill_id)
        } else if let Some(tool) = id.strip_prefix(MCP_PREFIX) {
            Self::Mcp(tool)
        } else {
            Self::Builtin(id)
        }
    }

    pub fn is_skill(self) -> bool {
        matches!(self, Self::Skill(_))
    }

    pub fn is_mcp(self) -> bool {
        matches!(self, Self::Mcp(_))
    }

    pub fn skill_id(self) -> Option<&'a str> {
        match self {
            Self::Skill(skill_id) => Some(skill_id),
            _ => None,
        }
    }
}

pub fn skill_capability_id(skill_id: &str) -> String {
    format!("{SKILL_PREFIX}{skill_id}")
}

pub fn mcp_capability_id(server_id: &str, tool: &str) -> String {
    format!("{MCP_PREFIX}{server_id}.{tool}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_their_constructors() {
        let skill = skill_capability_id("daily-brief");
        assert_eq!(
            CapabilityRef::parse(&skill),
            CapabilityRef::Skill("daily-brief")
        );
        let mcp = mcp_capability_id("files.local", "read");
        assert!(CapabilityRef::parse(&mcp).is_mcp());
        assert_eq!(
            CapabilityRef::parse("phantasi.search"),
            CapabilityRef::Builtin("phantasi.search")
        );
    }

    #[test]
    fn no_other_code_parses_the_id_prefixes() {
        fn visit(dir: &std::path::Path, offenders: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(&path, offenders);
                } else if path.extension().is_some_and(|ext| ext == "rs")
                    && !path.ends_with("capability/reference.rs")
                {
                    let source = std::fs::read_to_string(&path).unwrap();
                    for prefix in [SKILL_PREFIX, MCP_PREFIX] {
                        for method in ["starts_with", "strip_prefix"] {
                            if source.contains(&format!("{method}(\"{prefix}\")")) {
                                offenders.push(format!("{}: {method}({prefix})", path.display()));
                            }
                        }
                    }
                }
            }
        }
        let mut offenders = Vec::new();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        visit(&root, &mut offenders);
        assert!(
            offenders.is_empty(),
            "parse ids with CapabilityRef: {offenders:#?}"
        );
    }
}
