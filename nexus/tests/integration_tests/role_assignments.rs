// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Basic test for role assignments

use dropshot::test_util::ClientTestContext;
use futures::future::BoxFuture;
use futures::Future;
use futures::FutureExt;
use http::Method;
use http::StatusCode;
use lazy_static::lazy_static;
use nexus_test_utils::http_testing::AuthnMode;
use nexus_test_utils::http_testing::NexusRequest;
use nexus_test_utils::resource_helpers::create_organization;
use nexus_test_utils::resource_helpers::create_project;
use nexus_test_utils::ControlPlaneTestContext;
use nexus_test_utils_macros::nexus_test;
use omicron_common::api::external::ObjectIdentity;
use omicron_nexus::authn::USER_TEST_UNPRIVILEGED;
use omicron_nexus::authz;
use omicron_nexus::db::fixed_data;
use omicron_nexus::db::identity::Resource;
use omicron_nexus::db::model::DatabaseString;
use omicron_nexus::external_api::shared;
use omicron_nexus::external_api::views;

lazy_static! {
    /// Authentication mode used for testing
    // The role assignment APIs only support assigning roles to Silo users and
    // eventually other externally-visible things like service accounts and
    // groups.  They don't support assigning roles to built-in users.  So to
    // test enforcement, we'll need to use a silo user.  We could create our
    // own, but the facilities for doing that don't really exist.  Fortunately,
    // there's currently a corresponding silo user for every built-in user, so
    // we can just choose the one for the "unprivileged" user.
    //
    // This will all change when we have first-class facilities for creating
    // silo users because we won't ship the "privileged" and "unprivileged"
    // users and we won't create silo users for built-in users.  Hopefully this
    // happens soon!
    static ref AUTHN_TEST_USER: AuthnMode =
        AuthnMode::SiloUser(USER_TEST_UNPRIVILEGED.id);
}

/// Describes the role assignment test for a particular kind of resource
///
/// This trait essentially describes a test case that will be fed into
/// `run_test()`.  With the information provided by this trait, `run_test()`
/// will run a test sequence that:
///
/// - verifies initial conditions (usually: unprivileged user has no access)
/// - attempts to grant `ROLE` on the resource using an unprivileged user
///   (should fail)
/// - verifies initial conditions again
/// - grants the admin role on the resource using a privileged user (should work)
/// - verifies privileged conditions (usually: previously-unprivileged user now
///   has access)
/// - revokes the admin role on the resource using the newly-privileged user
/// - verifies the initial conditions again
///
/// All together, this verifies basic policy CRUD, plus that the corresponding
/// changes are enforced correctly.
///
/// This is all much simpler than it sounds.  The reason this is so abstract is
/// that the behavior is slightly different for Fleets and Silos for various
/// reasons described in their impls below.
trait RoleAssignmentTest {
    /// The type that's used to describe roles on this resource
    type RoleType: Clone
        + std::fmt::Debug
        + PartialEq
        + serde::Serialize
        + serde::de::DeserializeOwned
        + DatabaseString;

    /// The role to grant on this resource as part of the test sequence
    const ROLE: Self::RoleType;

    /// Whether this resource is always visible to unprivileged users
    const VISIBLE_TO_UNPRIVILEGED: bool;

    /// Returns the URL of the policy to be checked and updated by the test
    fn policy_url(&self) -> String;

    /// Verifies the system's behavior when accessing this resource as an
    /// unprivileged user when no policy has been applied to the resource
    ///
    /// (This usually means verifying that an unprivileged user cannot access
    /// the resource.)
    fn verify_initial<'a, 'b, 'c, 'd>(
        &'a self,
        client: &'b ClientTestContext,
        current_policy: &'c shared::Policy<Self::RoleType>,
    ) -> BoxFuture<'d, ()>
    where
        'a: 'd,
        'b: 'd,
        'c: 'd;

    /// Verifies the system's behavior when accessing this kind of resource as
    /// a user that started unprivileged and was granted role `Self::ROLE` on
    /// this resource
    ///
    /// (This usually means verifying that an unprivileged user who has been
    /// granted `ROLE` on this resource can access the resource.)
    fn verify_privileged<'a, 'b, 'c>(
        &'a self,
        client: &'b ClientTestContext,
    ) -> BoxFuture<'c, ()>
    where
        'a: 'c,
        'b: 'c;
}

#[nexus_test]
async fn test_role_assignments_fleet(cptestctx: &ControlPlaneTestContext) {
    // There's no operation to read the Fleet directly, so we list Sleds as a
    // proxy for something that requires Fleet-level "read" permission.
    const RESOURCE_URL: &'static str = "/hardware/sleds";

    struct FleetRoleAssignmentTest;
    impl RoleAssignmentTest for FleetRoleAssignmentTest {
        type RoleType = authz::FleetRoles;
        const ROLE: Self::RoleType = authz::FleetRoles::Admin;
        const VISIBLE_TO_UNPRIVILEGED: bool = true;
        fn policy_url(&self) -> String {
            String::from("/policy")
        }

        fn verify_initial<'a, 'b, 'c, 'd>(
            &'a self,
            client: &'b ClientTestContext,
            _current_policy: &'c shared::Policy<Self::RoleType>,
        ) -> BoxFuture<'d, ()>
        where
            'a: 'd,
            'b: 'd,
            'c: 'd,
        {
            async {
                // There's no operation to read the Fleet directly, so we list
                // Sleds as a proxy for something that requires Fleet-level
                // "read" permission.
                NexusRequest::expect_failure(
                    client,
                    StatusCode::FORBIDDEN,
                    Method::GET,
                    RESOURCE_URL,
                )
                .authn_as(AUTHN_TEST_USER.clone())
                .execute()
                .await
                .unwrap();
            }
            .boxed()
        }

        fn verify_privileged<'a, 'b, 'c>(
            &'a self,
            client: &'b ClientTestContext,
        ) -> BoxFuture<'c, ()>
        where
            'a: 'c,
            'b: 'c,
        {
            async {
                let _: dropshot::ResultsPage<views::Sled> =
                    NexusRequest::object_get(client, RESOURCE_URL)
                        .authn_as(AUTHN_TEST_USER.clone())
                        .execute()
                        .await
                        .unwrap()
                        .parsed_body()
                        .unwrap();
            }
            .boxed()
        }
    }

    let client = &cptestctx.external_client;
    run_test(client, FleetRoleAssignmentTest {}).await;
}

#[nexus_test]
async fn test_role_assignments_silo(cptestctx: &ControlPlaneTestContext) {
    struct SiloRoleAssignmentTest;
    impl RoleAssignmentTest for SiloRoleAssignmentTest {
        type RoleType = authz::SiloRoles;
        const ROLE: Self::RoleType = authz::SiloRoles::Admin;
        const VISIBLE_TO_UNPRIVILEGED: bool = true;
        fn policy_url(&self) -> String {
            format!(
                "/silos/{}/policy",
                fixed_data::silo::DEFAULT_SILO.identity().name.to_string()
            )
        }

        fn verify_initial<'a, 'b, 'c, 'd>(
            &'a self,
            _: &'b ClientTestContext,
            _current_policy: &'c shared::Policy<Self::RoleType>,
        ) -> BoxFuture<'d, ()>
        where
            'a: 'd,
            'b: 'd,
            'c: 'd,
        {
            async {
                // TODO-coverage TODO-security There is currently nothing that
                // requires the ability to modify a Silo.  Once there is, we
                // should test it here.
            }
            .boxed()
        }

        fn verify_privileged<'a, 'b, 'c>(
            &'a self,
            _: &'b ClientTestContext,
        ) -> BoxFuture<'c, ()>
        where
            'a: 'c,
            'b: 'c,
        {
            async {
                // TODO-coverage TODO-security There is currently nothing that
                // requires the ability to modify a Silo.  Once there is, we
                // should test it here.
            }
            .boxed()
        }
    }

    let client = &cptestctx.external_client;
    run_test(client, SiloRoleAssignmentTest {}).await;
}

#[nexus_test]
async fn test_role_assignments_organization(
    cptestctx: &ControlPlaneTestContext,
) {
    let client = &cptestctx.external_client;
    let org_name = "test-org";
    create_organization(client, org_name).await;
    let org_url = format!("/organizations/{}", org_name);

    struct OrganizationRoleAssignmentTest {
        org_name: String,
        org_url: String,
    }

    let test_case = OrganizationRoleAssignmentTest {
        org_name: String::from(org_name),
        org_url: org_url.clone(),
    };

    impl RoleAssignmentTest for OrganizationRoleAssignmentTest {
        type RoleType = authz::OrganizationRoles;
        const ROLE: Self::RoleType = authz::OrganizationRoles::Admin;
        const VISIBLE_TO_UNPRIVILEGED: bool = false;
        fn policy_url(&self) -> String {
            format!("{}/policy", self.org_url)
        }

        fn verify_initial<'a, 'b, 'c, 'd>(
            &'a self,
            client: &'b ClientTestContext,
            current_policy: &'c shared::Policy<Self::RoleType>,
        ) -> BoxFuture<'d, ()>
        where
            'a: 'd,
            'b: 'd,
            'c: 'd,
        {
            resource_initial_conditions(client, &self.org_url, current_policy)
                .boxed()
        }

        fn verify_privileged<'a, 'b, 'c>(
            &'a self,
            client: &'b ClientTestContext,
        ) -> BoxFuture<'c, ()>
        where
            'a: 'c,
            'b: 'c,
        {
            resource_privileged_conditions::<views::Organization>(
                client,
                &self.org_url,
                &self.org_name,
            )
            .boxed()
        }
    }

    run_test(client, test_case).await;
}

#[nexus_test]
async fn test_role_assignments_project(cptestctx: &ControlPlaneTestContext) {
    let client = &cptestctx.external_client;
    let org_name = "test-org";
    let project_name = "test-project";
    create_organization(client, org_name).await;
    create_project(client, org_name, project_name).await;
    let project_url =
        format!("/organizations/{}/projects/{}", org_name, project_name);

    struct ProjectRoleAssignmentTest {
        project_name: String,
        project_url: String,
        policy_url: String,
    }
    let test_case = ProjectRoleAssignmentTest {
        project_name: String::from(project_name),
        project_url: project_url.clone(),
        policy_url: format!("{}/policy", project_url),
    };
    impl RoleAssignmentTest for ProjectRoleAssignmentTest {
        type RoleType = authz::ProjectRoles;
        const ROLE: Self::RoleType = authz::ProjectRoles::Admin;
        const VISIBLE_TO_UNPRIVILEGED: bool = false;
        fn policy_url(&self) -> String {
            self.policy_url.clone()
        }

        fn verify_initial<'a, 'b, 'c, 'd>(
            &'a self,
            client: &'b ClientTestContext,
            current_policy: &'c shared::Policy<Self::RoleType>,
        ) -> BoxFuture<'d, ()>
        where
            'a: 'd,
            'b: 'd,
            'c: 'd,
        {
            resource_initial_conditions(
                client,
                &self.project_url,
                current_policy,
            )
            .boxed()
        }

        fn verify_privileged<'a, 'b, 'c>(
            &'a self,
            client: &'b ClientTestContext,
        ) -> BoxFuture<'c, ()>
        where
            'a: 'c,
            'b: 'c,
        {
            resource_privileged_conditions::<views::Project>(
                client,
                &self.project_url,
                &self.project_name,
            )
            .boxed()
        }
    }

    run_test(client, test_case).await;
}

/// Helper function for verifying the initial (unprivileged) conditions for most
/// resources
///
/// This is used for the Organization and Project tests today.  If we add
/// support for assigning roles on other kinds of resources, we'd likely use
/// this for those, too.  (It's Fleet and Silo that are special cases.)
fn resource_initial_conditions<'a, 'b, 'c, 'd, T>(
    client: &'a ClientTestContext,
    resource_url: &'b str,
    current_policy: &'c shared::Policy<T>,
) -> impl Future<Output = ()> + 'd
where
    'a: 'd,
    'b: 'd,
    'c: 'd,
    T: serde::de::DeserializeOwned,
{
    async move {
        // For these resources, the initial policy is totally empty.
        assert!(current_policy.role_assignments.is_empty());

        // Verify that the unprivileged user cannot access this resource.  This
        // is primarily tested in the separate "unauthorized" test, but we do it
        // here as a control to make sure that the "privileged conditions"
        // checks pass for the right reasons.
        NexusRequest::expect_failure(
            client,
            StatusCode::NOT_FOUND,
            Method::GET,
            resource_url,
        )
        .authn_as(AUTHN_TEST_USER.clone())
        .execute()
        .await
        .unwrap();
    }
}

/// Helper function for verifying the privileged conditions for most resources
///
/// This is used for the Organization and Project tests today.  If we add
/// support for assigning roles on other kinds of resources, we'd likely use
/// this for those, too.  (It's Fleet and Silo that are special cases.)
fn resource_privileged_conditions<'a, 'b, 'c, 'd, V>(
    client: &'a ClientTestContext,
    resource_url: &'b str,
    resource_name: &'c str,
) -> impl Future<Output = ()> + 'd
where
    'a: 'd,
    'b: 'd,
    'c: 'd,
    V: serde::de::DeserializeOwned + ObjectIdentity,
{
    async move {
        // Once granted access, a user ought to be able to fetch the resource.
        // (This is not really a policy test so we're not going to check all
        // possible actions.)
        let resource: V = NexusRequest::object_get(client, resource_url)
            .authn_as(AUTHN_TEST_USER.clone())
            .execute()
            .await
            .unwrap()
            .parsed_body()
            .unwrap();
        assert_eq!(resource.identity().name, resource_name);
    }
}

/// Helper function for running a role assignment test on the given resource
///
/// See [`RoleAssignmentTest`] for details.
// TODO-coverage A more comprehensive test would be useful when we have proper
// Silo users
async fn run_test<T: RoleAssignmentTest>(
    client: &ClientTestContext,
    test_case: T,
) {
    // Fetch the initial policy.
    let policy_url = test_case.policy_url();
    let initial_policy = policy_fetch::<T::RoleType>(client, &policy_url).await;

    // Verify the initial conditions.  Usually, this means the policy will be
    // empty and the unprivileged user cannot access this resource.
    test_case.verify_initial(client, &initial_policy).await;

    // Construct a new policy granting the unprivileged user access to this
    // resource.  This is a little ugly, but we don't have a way of creating
    // silo users yet and it's worth testing this.
    let mut new_policy = initial_policy.clone();
    let role_assignment = shared::RoleAssignment {
        identity_type: shared::IdentityType::SiloUser,
        identity_id: USER_TEST_UNPRIVILEGED.id,
        role_name: T::ROLE,
    };
    new_policy.role_assignments.push(role_assignment.clone());

    // Make sure the unprivileged user can't grant themselves access!
    // As with all authz failures, the error code depends on whether the user
    // should be able to even know that this resource exists.
    let expected_status = if T::VISIBLE_TO_UNPRIVILEGED {
        StatusCode::FORBIDDEN
    } else {
        StatusCode::NOT_FOUND
    };
    NexusRequest::expect_failure_with_body(
        client,
        expected_status,
        Method::PUT,
        &policy_url,
        &new_policy,
    )
    .authn_as(AUTHN_TEST_USER.clone())
    .execute()
    .await
    .unwrap();

    // Check that it really didn't work.  The policy did not change, and the
    // enforcement behavior did not change.
    let current_policy = policy_fetch::<T::RoleType>(client, &policy_url).await;
    assert_eq!(initial_policy, current_policy);
    test_case.verify_initial(client, &current_policy).await;

    // Okay, really grant them access.
    let mut updated_policy: shared::Policy<T::RoleType> =
        NexusRequest::object_put(client, &policy_url, Some(&new_policy))
            .authn_as(AuthnMode::PrivilegedUser)
            .execute()
            .await
            .unwrap()
            .parsed_body()
            .unwrap();
    new_policy.role_assignments.sort_by_key(|r| {
        (r.identity_id, r.role_name.to_database_string().to_owned())
    });
    updated_policy.role_assignments.sort_by_key(|r| {
        (r.identity_id, r.role_name.to_database_string().to_owned())
    });
    assert_eq!(updated_policy, new_policy);

    // Check that the policy reflects that.
    let current_policy = policy_fetch::<T::RoleType>(client, &policy_url).await;
    assert_eq!(
        current_policy.role_assignments.len(),
        initial_policy.role_assignments.len() + 1
    );
    let new_one = current_policy
        .role_assignments
        .iter()
        .find(|r| !initial_policy.role_assignments.contains(r))
        .expect("found no new role assignment that wasn't there before");
    assert_eq!(*new_one, role_assignment);

    // Check that the enforcement behavior reflects the change.  (This basically
    // means the so-called unprivileged user should be able to access this
    // resource now.)
    test_case.verify_privileged(client).await;

    // The way we've defined things, the unprivileged user ought to be able to
    // revoke their own access.
    let updated_policy: shared::Policy<T::RoleType> =
        NexusRequest::object_put(client, &policy_url, Some(&initial_policy))
            .authn_as(AUTHN_TEST_USER.clone())
            .execute()
            .await
            .unwrap()
            .parsed_body()
            .unwrap();
    assert_eq!(updated_policy, initial_policy);

    // Check that the policy reflects that.
    let current_policy = policy_fetch::<T::RoleType>(client, &policy_url).await;
    assert_eq!(current_policy, initial_policy);
    // Check that the enforcement behavior reflects the change.  (The
    // unprivileged user should not be able to access this any more.)
    test_case.verify_initial(client, &current_policy).await;
}

async fn policy_fetch<T: serde::de::DeserializeOwned>(
    client: &ClientTestContext,
    policy_url: &str,
) -> shared::Policy<T> {
    NexusRequest::object_get(client, policy_url)
        .authn_as(AuthnMode::PrivilegedUser)
        .execute()
        .await
        .unwrap()
        .parsed_body()
        .unwrap()
}