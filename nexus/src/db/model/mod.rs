// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structures stored to the database.

mod block_size;
mod bytecount;
mod console_session;
mod dataset;
mod dataset_kind;
mod digest;
mod disk;
mod disk_state;
mod generation;
mod global_image;
mod image;
mod instance;
mod instance_cpu_count;
mod instance_state;
mod ipv4net;
mod ipv6net;
mod l4_port_range;
mod macaddr;
mod name;
mod network_interface;
mod organization;
mod oximeter_info;
mod producer_endpoint;
mod project;
mod rack;
mod region;
mod role_assignment;
mod role_builtin;
mod silo;
mod silo_user;
mod sled;
mod snapshot;
mod ssh_key;
mod u16;
mod update_artifact;
mod user_builtin;
mod vni;
mod volume;
mod vpc;
mod vpc_firewall_rule;
mod vpc_route;
mod vpc_router;
mod vpc_subnet;
mod zpool;

pub use self::macaddr::*;
pub use self::u16::*;
pub use block_size::*;
pub use bytecount::*;
pub use console_session::*;
pub use dataset::*;
pub use dataset_kind::*;
pub use digest::*;
pub use disk::*;
pub use disk_state::*;
pub use generation::*;
pub use global_image::*;
pub use image::*;
pub use instance::*;
pub use instance_cpu_count::*;
pub use instance_state::*;
pub use ipv4net::*;
pub use ipv6net::*;
pub use l4_port_range::*;
pub use name::*;
pub use network_interface::*;
pub use organization::*;
pub use oximeter_info::*;
pub use producer_endpoint::*;
pub use project::*;
pub use rack::*;
pub use region::*;
pub use role_assignment::*;
pub use role_builtin::*;
pub use silo::*;
pub use silo_user::*;
pub use sled::*;
pub use snapshot::*;
pub use ssh_key::*;
pub use update_artifact::*;
pub use user_builtin::*;
pub use vni::*;
pub use volume::*;
pub use vpc::*;
pub use vpc_firewall_rule::*;
pub use vpc_route::*;
pub use vpc_router::*;
pub use vpc_subnet::*;
pub use zpool::*;

// TODO: The existence of both impl_enum_type and impl_enum_wrapper is a
// temporary state of affairs while we do the work of converting uses of
// impl_enum_wrapper to impl_enum_type. This is part of a broader initiative to
// move types out of the common crate into Nexus where possible. See
// https://github.com/oxidecomputer/omicron/issues/388

/// This macro implements serialization and deserialization of an enum type from
/// our database into our model types. This version wraps an enum imported from
/// the common crate in a struct so we can implement DB traits on it. We are
/// moving those enum definitions into this file and using impl_enum_type
/// instead, so eventually this macro will go away. See [`InstanceState`] for a
/// sample usage.
macro_rules! impl_enum_wrapper {
    (
        $(#[$enum_meta:meta])*
        pub struct $diesel_type:ident;

        $(#[$model_meta:meta])*
        pub struct $model_type:ident(pub $ext_type:ty);
        $($enum_item:ident => $sql_value:literal)+
    ) => {
        $(#[$enum_meta])*
        pub struct $diesel_type;

        $(#[$model_meta])*
        pub struct $model_type(pub $ext_type);

        impl ::diesel::serialize::ToSql<$diesel_type, ::diesel::pg::Pg> for $model_type {
            fn to_sql<'a>(
                &'a self,
                out: &mut ::diesel::serialize::Output<'a, '_, ::diesel::pg::Pg>,
            ) -> ::diesel::serialize::Result {
                match self.0 {
                    $(
                    <$ext_type>::$enum_item => {
                        out.write_all($sql_value)?
                    }
                    )*
                }
                Ok(::diesel::serialize::IsNull::No)
            }
        }

        impl ::diesel::deserialize::FromSql<$diesel_type, ::diesel::pg::Pg> for $model_type {
            fn from_sql(bytes: ::diesel::backend::RawValue<::diesel::pg::Pg>) -> ::diesel::deserialize::Result<Self> {
                match ::diesel::backend::RawValue::<::diesel::pg::Pg>::as_bytes(&bytes) {
                    $(
                    $sql_value => {
                        Ok($model_type(<$ext_type>::$enum_item))
                    }
                    )*
                    _ => {
                        Err(concat!("Unrecognized enum variant for ",
                                stringify!{$model_type})
                            .into())
                    }
                }
            }
        }
    }
}

pub(crate) use impl_enum_wrapper;

/// This macro implements serialization and deserialization of an enum type from
/// our database into our model types. See [`VpcRouterKindEnum`] and
/// [`VpcRouterKind`] for a sample usage
macro_rules! impl_enum_type {
    (
        $(#[$enum_meta:meta])*
        pub struct $diesel_type:ident;

        $(#[$model_meta:meta])*
        pub enum $model_type:ident;

        $($enum_item:ident => $sql_value:literal)+
    ) => {
        $(#[$enum_meta])*
        pub struct $diesel_type;

        $(#[$model_meta])*
        pub enum $model_type {
            $(
                $enum_item,
            )*
        }

        impl ::diesel::serialize::ToSql<$diesel_type, ::diesel::pg::Pg> for $model_type {
            fn to_sql<'a>(
                &'a self,
                out: &mut ::diesel::serialize::Output<'a, '_, ::diesel::pg::Pg>,
            ) -> ::diesel::serialize::Result {
                match self {
                    $(
                    $model_type::$enum_item => {
                        out.write_all($sql_value)?
                    }
                    )*
                }
                Ok(::diesel::serialize::IsNull::No)
            }
        }

        impl ::diesel::deserialize::FromSql<$diesel_type, ::diesel::pg::Pg> for $model_type {
            fn from_sql(bytes: ::diesel::backend::RawValue<::diesel::pg::Pg>) -> ::diesel::deserialize::Result<Self> {
                match ::diesel::backend::RawValue::<::diesel::pg::Pg>::as_bytes(&bytes) {
                    $(
                    $sql_value => {
                        Ok($model_type::$enum_item)
                    }
                    )*
                    _ => {
                        Err(concat!("Unrecognized enum variant for ",
                                stringify!{$model_type})
                            .into())
                    }
                }
            }
        }
    }
}

pub(crate) use impl_enum_type;

/// Describes a type that's represented in the database using a String
///
/// If you're reaching for this type, consider whether it'd be better to use an
/// enum in the database, along with `impl_enum_wrapper!` to define a
/// corresponding Rust type.  This trait is really intended for cases where
/// that's not possible (e.g., because the set of allowed values isn't the same
/// for all rows in the table).
///
/// For a given type, the impl of `DatabaseString` is potentially different than
/// the impl of `Serialize` (which is usually how the type appears in an API)
/// and the impl of `Display` (if the type even has one).  We could piggy-back
/// on `Display`, but it'd be a footgun in that someone might change `Display`
/// thinking it wouldn't have much impact, not realizing that it'd be breaking
/// an on-disk interface.
pub trait DatabaseString: Sized {
    type Error: std::fmt::Display;

    fn to_database_string(&self) -> &str;
    fn from_database_string(s: &str) -> Result<Self, Self::Error>;
}

/// Does some basic smoke checks on an impl of `DatabaseString`
///
/// This tests:
///
/// - that for every variant, if we serialize it and deserialize the result, we
///   get back the original variant
/// - that if we attempt to deserialize some _other_ input, we get back an error
/// - that the serialized form for each variant matches what's found in
///   `expected_output_file`.  This output file is generated by this test using
///   expectorate.
///
/// This cannot completely test the correctness of the implementation, but it
/// can catch some basic copy/paste errors and accidental compatibility
/// breakage.
#[cfg(test)]
pub fn test_database_string_impl<T, P>(expected_output_file: P)
where
    T: std::fmt::Debug + PartialEq + DatabaseString + strum::IntoEnumIterator,
    P: std::convert::AsRef<std::path::Path>,
{
    let mut output = String::new();
    let mut maxlen: Option<usize> = None;

    for variant in T::iter() {
        // Serialize the variant.  Verify that we can deserialize the thing we
        // just got back.
        let serialized = variant.to_database_string();
        let deserialized =
            T::from_database_string(serialized).unwrap_or_else(|_| {
                panic!(
                    "failed to deserialize the string {:?}, which we \
                    got by serializing {:?}",
                    serialized, variant
                )
            });
        assert_eq!(variant, deserialized);

        // Put the serialized form into "output".  At the end, we'll compare
        // this to the expected output.  This will fail if somebody has
        // incompatibly changed things.
        output.push_str(&format!(
            "variant {:?}: serialized form = {}\n",
            variant, serialized
        ));

        // Keep track of the maximum length of the serialized strings.  We'll
        // use this to construct an input to `from_database_string()` that was
        // not emitted by any of the variants' `to_database_string()` functions.
        match (maxlen, serialized.len()) {
            (None, newlen) => maxlen = Some(newlen),
            (Some(oldlen), newlen) if newlen > oldlen => maxlen = Some(newlen),
            _ => (),
        }
    }

    // Check that `from_database_string()` fails when given input that doesn't
    // match any of the variants' serialized forms.  We construct this input by
    // providing a string that's longer than all strings emitted by
    // `to_database_string()`.
    if let Some(maxlen) = maxlen {
        let input = String::from_utf8(vec![b'-'; maxlen + 1]).unwrap();
        T::from_database_string(&input)
            .expect_err("expected failure to deserialize unknown string");
    }

    expectorate::assert_contents(expected_output_file, &output);
}

#[cfg(test)]
mod tests {
    use super::VpcSubnet;
    use ipnetwork::Ipv4Network;
    use ipnetwork::Ipv6Network;
    use omicron_common::api::external::IdentityMetadataCreateParams;
    use omicron_common::api::external::Ipv4Net;
    use omicron_common::api::external::Ipv6Net;
    use std::net::IpAddr;
    use std::net::Ipv4Addr;
    use std::net::Ipv6Addr;
    use uuid::Uuid;

    #[test]
    fn test_vpc_subnet_check_requestable_addr() {
        let ipv4_block =
            Ipv4Net("192.168.0.0/16".parse::<Ipv4Network>().unwrap());
        let ipv6_block = Ipv6Net("fd00::/48".parse::<Ipv6Network>().unwrap());
        let identity = IdentityMetadataCreateParams {
            name: "net-test-vpc".parse().unwrap(),
            description: "A test VPC".parse().unwrap(),
        };
        let subnet = VpcSubnet::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            identity,
            ipv4_block,
            ipv6_block,
        );
        // Within subnet
        assert!(subnet
            .check_requestable_addr(IpAddr::from(Ipv4Addr::new(
                192, 168, 1, 10
            )))
            .is_ok());
        // Network address is reserved
        assert!(subnet
            .check_requestable_addr(IpAddr::from(Ipv4Addr::new(192, 168, 0, 0)))
            .is_err());
        // Broadcast address is reserved
        assert!(subnet
            .check_requestable_addr(IpAddr::from(Ipv4Addr::new(
                192, 168, 255, 255
            )))
            .is_err());
        // Within subnet, but reserved
        assert!(subnet
            .check_requestable_addr(IpAddr::from(Ipv4Addr::new(192, 168, 0, 1)))
            .is_err());
        // Not within subnet
        assert!(subnet
            .check_requestable_addr(IpAddr::from(Ipv4Addr::new(192, 160, 1, 1)))
            .is_err());

        // Within subnet
        assert!(subnet
            .check_requestable_addr(IpAddr::from(Ipv6Addr::new(
                0xfd00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10
            )))
            .is_ok());
        // Within subnet, but reserved
        assert!(subnet
            .check_requestable_addr(IpAddr::from(Ipv6Addr::new(
                0xfd00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01
            )))
            .is_err());
        // Not within subnet
        assert!(subnet
            .check_requestable_addr(IpAddr::from(Ipv6Addr::new(
                0xfc00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10
            )))
            .is_err());
    }

    #[test]
    fn test_ipv6_net_random_subnet() {
        let base = super::Ipv6Net(Ipv6Net(
            "fd00::/48".parse::<Ipv6Network>().unwrap(),
        ));
        assert!(
            base.random_subnet(8).is_none(),
            "random_subnet() should fail when prefix is less than the base prefix"
        );
        assert!(
            base.random_subnet(130).is_none(),
            "random_subnet() should fail when prefix is greater than 128"
        );
        let subnet = base.random_subnet(64).unwrap();
        assert_eq!(
            subnet.prefix(),
            64,
            "random_subnet() returned an incorrect prefix"
        );
        let octets = subnet.network().octets();
        const EXPECTED_RANDOM_BYTES: [u8; 8] = [253, 0, 0, 0, 0, 0, 111, 127];
        assert_eq!(octets[..8], EXPECTED_RANDOM_BYTES);
        assert!(
            octets[8..].iter().all(|x| *x == 0),
            "Host address portion should be 0"
        );
        assert!(
            base.is_supernet_of(subnet.0 .0),
            "random_subnet should generate an actual subnet"
        );
        assert_eq!(base.random_subnet(base.prefix()), Some(base));
    }

    #[test]
    fn test_ip_subnet_check_requestable_address() {
        let subnet = super::Ipv4Net(Ipv4Net("192.168.0.0/16".parse().unwrap()));
        assert!(subnet.check_requestable_addr("192.168.0.10".parse().unwrap()));
        assert!(subnet.check_requestable_addr("192.168.1.0".parse().unwrap()));
        assert!(!subnet.check_requestable_addr("192.168.0.0".parse().unwrap()));
        assert!(subnet.check_requestable_addr("192.168.0.255".parse().unwrap()));
        assert!(
            !subnet.check_requestable_addr("192.168.255.255".parse().unwrap())
        );

        let subnet = super::Ipv6Net(Ipv6Net("fd00::/64".parse().unwrap()));
        assert!(subnet.check_requestable_addr("fd00::a".parse().unwrap()));
        assert!(!subnet.check_requestable_addr("fd00::1".parse().unwrap()));
        assert!(subnet.check_requestable_addr("fd00::1:1".parse().unwrap()));
    }
}