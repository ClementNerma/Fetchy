use std::fmt;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

macro_rules! supported_platforms {
    ($cfg_name: ident as $enum_name: ident => $($value: ident),+) => {
        ::paste::paste! {
            #[allow(non_camel_case_types)]
            #[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
supported_platforms!(target_os as System => linux);

// Platform-dependent value
#[derive(Serialize, Deserialize)]
pub struct PlatformDependent<T>(pub Vec<(CpuArch, System, T)>);

impl<T> PlatformDependent<T> {
    pub fn get_for_current_platform(&self) -> Result<&T> {
        for (arch, os, value) in &self.0 {
            if *arch == CPU_ARCH && *os == SYSTEM {
                return Ok(value);
            }
        }

        bail!("No value found for current platform ({CPU_ARCH}, {SYSTEM})",);
    }
}
