//! One id type per kind of thing the ledger stores. They are all UUIDs
//! underneath, but a function that takes an `AccountId` cannot be handed a
//! `PotId` by mistake, and an id of the wrong kind cannot be compared with
//! or looked up as another. On the wire and in the database each is just
//! the UUID it wraps.

use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// A fresh, random id.
            pub fn generate() -> Self {
                $name(Uuid::new_v4())
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                $name(uuid)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Uuid {
                id.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map($name)
            }
        }
    };
}

id_type!(AccountId);
id_type!(EntryId);
id_type!(EntryPartId);
id_type!(PotId);
id_type!(AllocationId);
id_type!(ValuationId);
id_type!(ImportId);
id_type!(QueueRowId);
id_type!(MatchId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_the_uuid_it_wraps_on_the_wire_and_in_text() {
        let id = AccountId::generate();
        let text = id.to_string();
        assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{text}\""));
        assert_eq!(text.parse::<AccountId>().unwrap(), id);
        assert_eq!(AccountId::from(Uuid::from(id)), id);
    }

    #[test]
    fn ids_of_different_kinds_are_different_types_even_for_the_same_uuid() {
        let uuid = Uuid::new_v4();
        // Only compiles because each side is compared with its own kind.
        assert_eq!(AccountId::from(uuid), AccountId::from(uuid));
        assert_eq!(PotId::from(uuid), PotId::from(uuid));
    }
}
