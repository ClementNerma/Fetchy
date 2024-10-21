use std::{collections::HashMap, fmt, marker::PhantomData, ops::Deref};

use anyhow::{Context, Result};
use serde::{
    de::{SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};

macro_rules! supported_platforms {
    ($cfg_name: ident as $enum_name: ident => $($value: ident),+) => {
        ::paste::paste! {
            #[allow(non_camel_case_types)]
            #[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
            pub enum $enum_name {
                $( $value ),+
            }

            impl fmt::Display for $enum_name {
                fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
                    match self {
                        $( Self::$value => write!(f, stringify!($value)), )+
                    }
                }
            }

            $(
                #[cfg($cfg_name = $value:snake)]
                pub static [<$enum_name:snake:upper>]: $enum_name = $enum_name::$value;
            )+
        }
    };
}

// List of all supported CPU architectures
supported_platforms!(target_arch as CpuArch => x86_64, aarch64);

// List of all supported target OSes
supported_platforms!(target_os as System => linux, windows);

// Platform-dependent value
#[derive(Debug, Clone)]
pub struct PlatformDependent<T>(HashMap<(System, CpuArch), T>);

impl<T> PlatformDependent<T> {
    // TODO: ensure they are no clashing entries
    pub fn new(entries: impl IntoIterator<Item = PlatformDependentEntry<T>>) -> Self {
        Self(
            entries
                .into_iter()
                .map(|entry| {
                    let PlatformDependentEntry {
                        system,
                        cpu_arch,
                        value,
                    } = entry;

                    ((system, cpu_arch), value)
                })
                .collect(),
        )
    }

    pub fn get_for_current_platform(&self) -> Result<&T> {
        self.0
            .get(&(SYSTEM, CPU_ARCH))
            .with_context(|| format!("No value found for current platform ({CPU_ARCH}, {SYSTEM})"))
    }
}

impl<T> Deref for PlatformDependent<T> {
    type Target = HashMap<(System, CpuArch), T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: Serialize> Serialize for PlatformDependent<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self(entries) = self;

        serializer.collect_seq(
            entries
                .iter()
                .map(|((system, cpu_arch), value)| (system, cpu_arch, value)),
        )
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for PlatformDependent<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MapVisitor<T> {
            marker: PhantomData<T>,
        }

        impl<'de, T: Deserialize<'de>> Visitor<'de> for MapVisitor<T> {
            type Value = PlatformDependent<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an array")
            }

            #[inline]
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut out = HashMap::new();

                while let Some((system, cpu_arch, value)) =
                    seq.next_element::<(System, CpuArch, T)>()?
                {
                    out.insert((system, cpu_arch), value);
                }

                Ok(PlatformDependent(out))
            }
        }

        let visitor = MapVisitor {
            marker: PhantomData,
        };

        deserializer.deserialize_seq(visitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformDependentEntry<T> {
    pub system: System,
    pub cpu_arch: CpuArch,
    pub value: T,
}

impl<T> PlatformDependentEntry<T> {
    pub fn new(system: System, cpu_arch: CpuArch, value: T) -> Self {
        Self {
            system,
            cpu_arch,
            value,
        }
    }
}
