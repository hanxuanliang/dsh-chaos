use crate::{CollabError, Result};

macro_rules! string_id {
    ($name:ident, $label:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) struct $name(String);

        impl $name {
            pub(crate) fn parse(value: &str) -> Result<Self> {
                if value.trim().is_empty() {
                    return Err(CollabError::InvalidArgument(
                        concat!($label, " must not be blank").into(),
                    ));
                }
                Ok(Self(value.to_owned()))
            }

            pub(crate) fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

string_id!(ActorId, "actor_id");
string_id!(ThreadId, "thread_target_id");
