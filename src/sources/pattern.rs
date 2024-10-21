use std::{ops::Deref, str::FromStr};

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Required as 'regex' doesn't support serde
#[derive(Debug, Clone)]
pub struct Pattern(pub Regex);

impl Deref for Pattern {
    type Target = Regex;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for Pattern {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Pattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf = String::deserialize(deserializer)?;
        Regex::new(&buf).map(Self).map_err(serde::de::Error::custom)
    }
}

impl FromStr for Pattern {
    type Err = regex::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Regex::new(s).map(Self)
    }
}
