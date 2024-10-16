use regex::Regex;
use serde::{
    de::{Error, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};

#[derive(Debug, Clone)]
pub struct Pattern {
    pub source: String,
    pub regex: Regex,
}

impl Pattern {
    pub fn parse(input: &str) -> Result<Self, String> {
        Ok(Pattern {
            source: input.to_owned(),
            regex: Regex::new(input)
                .map_err(|err| format!("Failed to parse regex ({input}): {err}"))?,
        })
    }
}

impl Serialize for Pattern {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.source)
    }
}

struct RegexVisitor;

impl Visitor<'_> for RegexVisitor {
    type Value = Pattern;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(" a rust regex")
    }

    fn visit_str<E>(self, str: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        let regex = Regex::new(str)
            .map_err(|err| E::custom(format!("Failed to parse regex ({str}): {err}")))?;

        Ok(Pattern {
            source: str.to_owned(),
            regex,
        })
    }
}

impl<'de> Deserialize<'de> for Pattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(RegexVisitor)
    }
}
