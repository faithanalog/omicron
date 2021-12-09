// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/*!
 * Smoke tests against the API server
 *
 * This file defines a very basic set of tests against the API.
 * TODO-coverage add test for racks, sleds
 */

use dropshot::test_util::iter_collection;
use dropshot::test_util::object_get;
use dropshot::test_util::objects_list_page;
use dropshot::test_util::read_json;
use dropshot::test_util::ClientTestContext;
use dropshot::HttpErrorResponseBody;
use http::method::Method;
use http::StatusCode;
use omicron_common::api::external::IdentityMetadataCreateParams;
use omicron_common::api::external::IdentityMetadataUpdateParams;
use omicron_common::api::external::Name;
use omicron_nexus::external_api::{
    params,
    views::{Project, Sled},
};
use serde::Serialize;
use uuid::Uuid;

use super::common;
use common::http_testing::AuthnMode;
use common::http_testing::NexusRequest;
use common::http_testing::RequestBuilder;
use common::resource_helpers::create_organization;
use common::resource_helpers::create_project;
use common::start_sled_agent;
use common::test_setup;

#[tokio::test]
async fn test_basic_failures() {
    let testctx = test_setup("basic_failures").await;
    let client = &testctx.external_client;

    let org_name = "test-org";
    create_organization(&client, &org_name).await;

    /* Error case: GET /nonexistent (a path with no route at all) */
    let error = client
        .make_request(
            Method::GET,
            "/nonexistent",
            None as Option<()>,
            StatusCode::NOT_FOUND,
        )
        .await
        .expect_err("expected error");
    assert_eq!("Not Found", error.message);

    /*
     * Error case: GET /organizations/test-org/projects/nonexistent (a possible
     * value that does not exist inside a collection that does exist)
     */
    let error = client
        .make_request(
            Method::GET,
            "/organizations/test-org/projects/nonexistent",
            None as Option<()>,
            StatusCode::NOT_FOUND,
        )
        .await
        .expect_err("expected error");
    assert_eq!("not found: project with name \"nonexistent\"", error.message);

    /*
     * Error case: GET /organizations/test-org/projects/-invalid-name
     * TODO-correctness is 400 the right error code here or is 404 more
     * appropriate?
     */
    let error = client
        .make_request(
            Method::GET,
            "/organizations/test-org/projects/-invalid-name",
            None as Option<()>,
            StatusCode::BAD_REQUEST,
        )
        .await
        .expect_err("expected error");
    assert_eq!(
        "bad parameter in URL path: name must begin with an ASCII lowercase \
         character",
        error.message
    );

    /* Error case: PUT /organizations/test-org/projects */
    let error = client
        .make_request(
            Method::PUT,
            "/organizations/test-org/projects",
            None as Option<()>,
            StatusCode::METHOD_NOT_ALLOWED,
        )
        .await
        .expect_err("expected error");
    assert_eq!("Method Not Allowed", error.message);

    /* Error case: DELETE /organizations/test-org/projects */
    let error = client
        .make_request(
            Method::DELETE,
            "/organizations/test-org/projects",
            None as Option<()>,
            StatusCode::METHOD_NOT_ALLOWED,
        )
        .await
        .expect_err("expected error");
    assert_eq!("Method Not Allowed", error.message);

    /* Error case: list instances in a nonexistent project. */
    let error = client
        .make_request_with_body(
            Method::GET,
            "/organizations/test-org/projects/nonexistent/instances",
            "".into(),
            StatusCode::NOT_FOUND,
        )
        .await
        .expect_err("expected error");
    assert_eq!("not found: project with name \"nonexistent\"", error.message);

    /* Error case: fetch an instance in a nonexistent project. */
    let error = client
        .make_request_with_body(
            Method::GET,
            "/organizations/test-org/projects/nonexistent/instances/my-instance",
            "".into(),
            StatusCode::NOT_FOUND,
        )
        .await
        .expect_err("expected error");
    assert_eq!("not found: project with name \"nonexistent\"", error.message);

    /* Error case: fetch an instance with an invalid name. */
    let error = client
        .make_request_with_body(
            Method::GET,
            "/organizations/test-org/projects/nonexistent/instances/my_instance",
            "".into(),
            StatusCode::BAD_REQUEST,
        )
        .await
        .expect_err("expected error");
    assert_eq!(
        "bad parameter in URL path: name contains invalid character: \"_\" \
         (allowed characters are lowercase ASCII, digits, and \"-\")",
        error.message
    );

    /* Error case: delete an instance with an invalid name. */
    let error = client
        .make_request_with_body(
            Method::DELETE,
            "/organizations/test-org/projects/nonexistent/instances/my_instance",
            "".into(),
            StatusCode::BAD_REQUEST,
        )
        .await
        .expect_err("expected error");
    assert_eq!(
        "bad parameter in URL path: name contains invalid character: \"_\" \
         (allowed characters are lowercase ASCII, digits, and \"-\")",
        error.message
    );

    testctx.teardown().await;
}

#[tokio::test]
async fn test_projects_basic() {
    let testctx = test_setup("test_projects").await;
    let client = &testctx.external_client;

    let org_name = "test-org";
    create_organization(&client, &org_name).await;

    /*
     * Verify that there are no projects to begin with.
     */
    let projects_url = "/organizations/test-org/projects";
    let projects = projects_list(&client, &projects_url).await;
    assert_eq!(0, projects.len());

    /*
     * Create three projects used by the rest of this test.
     */
    let projects_to_create = vec!["simproject1", "simproject2", "simproject3"];
    let new_project_ids = {
        let mut project_ids: Vec<Uuid> = Vec::new();
        for project_name in projects_to_create {
            let new_project: Project = NexusRequest::objects_post(
                client,
                projects_url,
                &params::ProjectCreate {
                    identity: IdentityMetadataCreateParams {
                        name: project_name.parse().unwrap(),
                        description: String::from(
                            "<auto-generated by test suite>",
                        ),
                    },
                },
            )
            .authn_as(AuthnMode::PrivilegedUser)
            .execute()
            .await
            .expect("failed to make request")
            .parsed_body()
            .unwrap();
            assert_eq!(new_project.identity.name.as_str(), project_name);
            assert_eq!(
                new_project.identity.description,
                String::from("<auto-generated by test suite>")
            );
            project_ids.push(new_project.identity.id);
        }

        project_ids
    };

    /*
     * Error case: GET /organizations/test-org/projects/simproject1/nonexistent
     * (a path that does not exist beneath a resource that does exist)
     */
    let error = client
        .make_request_error(
            Method::GET,
            "/organizations/test-org/projects/simproject1/nonexistent",
            StatusCode::NOT_FOUND,
        )
        .await;
    assert_eq!("Not Found", error.message);

    /*
     * Basic GET /organizations/test-org/projects now that we've created a few.
     * TODO-coverage: pagination
     * TODO-coverage: marker even without pagination
     */
    let initial_projects = projects_list(&client, &projects_url).await;
    assert_eq!(initial_projects.len(), 3);
    assert_eq!(initial_projects[0].identity.id, new_project_ids[0]);
    assert_eq!(initial_projects[0].identity.name, "simproject1");
    assert!(initial_projects[0].identity.description.len() > 0);
    assert_eq!(initial_projects[1].identity.id, new_project_ids[1]);
    assert_eq!(initial_projects[1].identity.name, "simproject2");
    assert!(initial_projects[1].identity.description.len() > 0);
    assert_eq!(initial_projects[2].identity.id, new_project_ids[2]);
    assert_eq!(initial_projects[2].identity.name, "simproject3");
    assert!(initial_projects[2].identity.description.len() > 0);

    /*
     * Basic test of out-of-the-box GET
     * /organizations/test-org/projects/simproject2
     */
    let project =
        project_get(&client, "/organizations/test-org/projects/simproject2")
            .await;
    let expected = &initial_projects[1];
    assert_eq!(project.identity.id, expected.identity.id);
    assert_eq!(project.identity.name, expected.identity.name);
    assert_eq!(project.identity.description, expected.identity.description);
    assert!(project.identity.description.len() > 0);

    /*
     * Delete "simproject2".  We'll make sure that's reflected in the other
     * requests.
     */
    client
        .make_request_no_body(
            Method::DELETE,
            "/organizations/test-org/projects/simproject2",
            StatusCode::NO_CONTENT,
        )
        .await
        .expect("expected success");

    /*
     * Having deleted "simproject2", verify "GET", "PUT", and "DELETE" on
     * "/organizations/test-org/projects/simproject2".
     */
    client
        .make_request_error(
            Method::GET,
            "/organizations/test-org/projects/simproject2",
            StatusCode::NOT_FOUND,
        )
        .await;
    client
        .make_request_error(
            Method::DELETE,
            "/organizations/test-org/projects/simproject2",
            StatusCode::NOT_FOUND,
        )
        .await;
    client
        .make_request_error_body(
            Method::PUT,
            "/organizations/test-org/projects/simproject2",
            params::ProjectUpdate {
                identity: IdentityMetadataUpdateParams {
                    name: None,
                    description: None,
                },
            },
            StatusCode::NOT_FOUND,
        )
        .await;

    /*
     * Similarly, verify "GET /organizations/test-org/projects"
     */
    let expected_projects: Vec<&Project> = initial_projects
        .iter()
        .filter(|p| p.identity.name != "simproject2")
        .collect();
    let new_projects =
        projects_list(&client, "/organizations/test-org/projects").await;
    assert_eq!(new_projects.len(), expected_projects.len());
    assert_eq!(new_projects[0].identity.id, expected_projects[0].identity.id);
    assert_eq!(
        new_projects[0].identity.name,
        expected_projects[0].identity.name
    );
    assert_eq!(
        new_projects[0].identity.description,
        expected_projects[0].identity.description
    );
    assert_eq!(new_projects[1].identity.id, expected_projects[1].identity.id);
    assert_eq!(
        new_projects[1].identity.name,
        expected_projects[1].identity.name
    );
    assert_eq!(
        new_projects[1].identity.description,
        expected_projects[1].identity.description
    );

    /*
     * Update "simproject3".  We'll make sure that's reflected in the other
     * requests.
     */
    let project_update = params::ProjectUpdate {
        identity: IdentityMetadataUpdateParams {
            name: None,
            description: Some("Li'l lightnin'".to_string()),
        },
    };
    let mut response = client
        .make_request(
            Method::PUT,
            "/organizations/test-org/projects/simproject3",
            Some(project_update),
            StatusCode::OK,
        )
        .await
        .expect("expected success");
    let project: Project = read_json(&mut response).await;
    assert_eq!(project.identity.id, new_project_ids[2]);
    assert_eq!(project.identity.name, "simproject3");
    assert_eq!(project.identity.description, "Li'l lightnin'");

    let expected = project;
    let project =
        project_get(&client, "/organizations/test-org/projects/simproject3")
            .await;
    assert_eq!(project.identity.name, expected.identity.name);
    assert_eq!(project.identity.description, expected.identity.description);
    assert_eq!(project.identity.description, "Li'l lightnin'");

    /*
     * Update "simproject3" in a way that changes its name.  This is a deeper
     * operation under the hood.  This case also exercises changes to multiple
     * fields in one request.
     */
    let project_update = params::ProjectUpdate {
        identity: IdentityMetadataUpdateParams {
            name: Some("lil-lightnin".parse().unwrap()),
            description: Some("little lightning".to_string()),
        },
    };
    let mut response = client
        .make_request(
            Method::PUT,
            "/organizations/test-org/projects/simproject3",
            Some(project_update),
            StatusCode::OK,
        )
        .await
        .expect("failed to make request to server");
    let project: Project = read_json(&mut response).await;
    assert_eq!(project.identity.id, new_project_ids[2]);
    assert_eq!(project.identity.name, "lil-lightnin");
    assert_eq!(project.identity.description, "little lightning");

    client
        .make_request_error(
            Method::GET,
            "/organizations/test-org/projects/simproject3",
            StatusCode::NOT_FOUND,
        )
        .await;

    /*
     * Try to create a project with a name that conflicts with an existing one.
     */
    let project_create = params::ProjectCreate {
        identity: IdentityMetadataCreateParams {
            name: "simproject1".parse().unwrap(),
            description: "a duplicate of simproject1".to_string(),
        },
    };
    let error = NexusRequest::new(
        RequestBuilder::new(client, Method::POST, &projects_url)
            .body(Some(&project_create))
            .expect_status(Some(StatusCode::BAD_REQUEST)),
    )
    .authn_as(AuthnMode::PrivilegedUser)
    .execute()
    .await
    .expect("expected request to fail")
    .parsed_body::<HttpErrorResponseBody>()
    .unwrap();
    assert_eq!("already exists: project \"simproject1\"", error.message);

    /*
     * Try to create a project with an unsupported name.
     * TODO-polish why doesn't serde include the field name in this error?
     */
    #[derive(Serialize)]
    struct BadProject {
        name: &'static str,
        description: &'static str,
    }
    let error = NexusRequest::new(
        RequestBuilder::new(client, Method::POST, &projects_url)
            .body(Some(&BadProject {
                name: "sim_project",
                description: "underscore",
            }))
            .expect_status(Some(StatusCode::BAD_REQUEST)),
    )
    .authn_as(AuthnMode::PrivilegedUser)
    .execute()
    .await
    .expect("expected request to fail")
    .parsed_body::<HttpErrorResponseBody>()
    .unwrap();
    assert!(error.message.starts_with(
        "unable to parse body: name contains invalid character: \"_\" \
         (allowed characters are lowercase ASCII, digits, and \"-\""
    ));

    /*
     * Now, really do create another project.
     */
    let project_create = params::ProjectCreate {
        identity: IdentityMetadataCreateParams {
            name: "honor-roller".parse().unwrap(),
            description: "a soapbox racer".to_string(),
        },
    };
    let project: Project =
        NexusRequest::objects_post(client, projects_url, &project_create)
            .authn_as(AuthnMode::PrivilegedUser)
            .execute()
            .await
            .expect("failed to make request")
            .parsed_body()
            .unwrap();
    assert_eq!(project.identity.name, "honor-roller");
    assert_eq!(project.identity.description, "a soapbox racer");

    /*
     * Attempt to create a project without authenticating or without privileges.
     */
    RequestBuilder::new(client, Method::POST, &projects_url)
        .expect_status(Some(StatusCode::UNAUTHORIZED))
        .body(Some(&project_create))
        .execute()
        .await
        .expect("expected request to fail");
    NexusRequest::new(
        RequestBuilder::new(client, Method::POST, &projects_url)
            .body(Some(&project_create))
            .expect_status(Some(StatusCode::FORBIDDEN)),
    )
    .authn_as(AuthnMode::UnprivilegedUser)
    .execute()
    .await
    .expect("expected request to fail");

    /*
     * List projects again and verify all of our changes.  We should have:
     *
     * - "honor-roller" with description "a soapbox racer"
     * - "lil-lightnin" with description "little lightning"
     * - "simproject1", same as when it was created.
     */
    let projects = projects_list(&client, &projects_url).await;
    assert_eq!(projects.len(), 3);
    assert_eq!(projects[0].identity.name, "honor-roller");
    assert_eq!(projects[0].identity.description, "a soapbox racer");
    assert_eq!(projects[1].identity.name, "lil-lightnin");
    assert_eq!(projects[1].identity.description, "little lightning");
    assert_eq!(projects[2].identity.name, "simproject1");
    assert!(projects[2].identity.description.len() > 0);

    testctx.teardown().await;
}

#[tokio::test]
async fn test_projects_list() {
    let testctx = test_setup("test_projects_list").await;
    let client = &testctx.external_client;

    let org_name = "test-org";
    create_organization(&client, &org_name).await;

    /* Verify that there are no projects to begin with. */
    let projects_url = "/organizations/test-org/projects";
    assert_eq!(projects_list(&client, &projects_url).await.len(), 0);

    /* Create a large number of projects that we can page through. */
    let projects_total = 10;
    let projects_subset = 3;
    let mut projects_created = Vec::with_capacity(projects_total);
    for _ in 0..projects_total {
        /*
         * We'll use uuids for the names to make sure that works, and that we
         * can paginate through by _name_ even though the names happen to be
         * uuids.  Names have to start with a letter, though, so we've got to
         * make sure our uuid has one.
         */
        let mut name = Uuid::new_v4().to_string();
        name.replace_range(0..1, "a");
        let project = create_project(&client, org_name, &name).await;
        projects_created.push(project.identity);
    }

    let project_names_by_name = {
        let mut clone = projects_created.clone();
        clone.sort_by_key(|v| v.name.clone());
        assert_ne!(clone, projects_created);
        clone.iter().map(|v| v.name.clone()).collect::<Vec<Name>>()
    };

    let project_names_by_id = {
        let mut clone = projects_created.clone();
        clone.sort_by_key(|v| v.id);
        assert_ne!(clone, projects_created);
        clone.iter().map(|v| v.id).collect::<Vec<Uuid>>()
    };

    /*
     * Page through all the projects in the default order, which should be in
     * increasing order of name.
     */
    let found_projects_by_name =
        iter_collection::<Project>(&client, projects_url, "", projects_subset)
            .await
            .0;
    assert_eq!(found_projects_by_name.len(), project_names_by_name.len());
    assert_eq!(
        project_names_by_name,
        found_projects_by_name
            .iter()
            .map(|v| v.identity.name.clone())
            .collect::<Vec<Name>>()
    );

    /*
     * Page through all the projects in ascending order by name, which should be
     * the same as above.
     */
    let found_projects_by_name = iter_collection::<Project>(
        &client,
        projects_url,
        "sort_by=name-ascending",
        projects_subset,
    )
    .await
    .0;
    assert_eq!(found_projects_by_name.len(), project_names_by_name.len());
    assert_eq!(
        project_names_by_name,
        found_projects_by_name
            .iter()
            .map(|v| v.identity.name.clone())
            .collect::<Vec<Name>>()
    );

    /*
     * Page through all the projects in descending order by name, which should be
     * the reverse of the above.
     */
    let mut found_projects_by_name = iter_collection::<Project>(
        &client,
        projects_url,
        "sort_by=name-descending",
        projects_subset,
    )
    .await
    .0;
    assert_eq!(found_projects_by_name.len(), project_names_by_name.len());
    found_projects_by_name.reverse();
    assert_eq!(
        project_names_by_name,
        found_projects_by_name
            .iter()
            .map(|v| v.identity.name.clone())
            .collect::<Vec<Name>>()
    );

    /*
     * Page through the projects in ascending order by id.
     */
    let found_projects_by_id = iter_collection::<Project>(
        &client,
        projects_url,
        "sort_by=id-ascending",
        projects_subset,
    )
    .await
    .0;
    assert_eq!(found_projects_by_id.len(), project_names_by_id.len());
    assert_eq!(
        project_names_by_id,
        found_projects_by_id
            .iter()
            .map(|v| v.identity.id)
            .collect::<Vec<Uuid>>()
    );

    testctx.teardown().await;
}

#[tokio::test]
async fn test_sleds_list() {
    let testctx = test_setup("test_sleds_list").await;
    let client = &testctx.external_client;

    /* Verify that there is one sled to begin with. */
    let sleds_url = "/hardware/sleds";
    assert_eq!(sleds_list(&client, &sleds_url).await.len(), 1);

    /* Now start a few more sled agents. */
    let nsleds = 3;
    let mut sas = Vec::with_capacity(nsleds);
    for _ in 0..nsleds {
        let sa_id = Uuid::new_v4();
        let log = testctx.logctx.log.new(o!( "sled_id" => sa_id.to_string() ));
        let addr = testctx.server.http_server_internal.local_addr();
        sas.push(start_sled_agent(log, addr, sa_id).await.unwrap());
    }

    /* List sleds again. */
    let sleds_found = sleds_list(&client, &sleds_url).await;
    assert_eq!(sleds_found.len(), nsleds + 1);

    let sledids_found =
        sleds_found.iter().map(|sv| sv.identity.id).collect::<Vec<Uuid>>();
    let mut sledids_found_sorted = sledids_found.clone();
    sledids_found_sorted.sort();
    assert_eq!(sledids_found, sledids_found_sorted);

    /* Tear down the agents. */
    for sa in sas {
        sa.http_server.close().await.unwrap();
    }

    testctx.teardown().await;
}

async fn projects_list(
    client: &ClientTestContext,
    projects_url: &str,
) -> Vec<Project> {
    objects_list_page::<Project>(client, projects_url).await.items
}

async fn project_get(client: &ClientTestContext, project_url: &str) -> Project {
    object_get::<Project>(client, project_url).await
}

async fn sleds_list(client: &ClientTestContext, sleds_url: &str) -> Vec<Sled> {
    objects_list_page::<Sled>(client, sleds_url).await.items
}