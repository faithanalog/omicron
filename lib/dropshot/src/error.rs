/*!
 * Generic server error handling facilities
 *
 * Error handling in an API
 * ------------------------
 *
 * Our approach for managing errors within the API server balances several
 * goals:
 *
 * * Every incoming HTTP request should conclude with a response, which is
 *   either successful (200-level or 300-level status code) or a failure
 *   (400-level for client errors, 500-level for server errors).
 * * There are several different sources of errors within an API server:
 *     * The HTTP layer of the server may generate an error.  In this case, it
 *       may be just as easy to generate the appropriate HTTP response (with a
 *       400-level or 500-level status code) as it would be to generate an Error
 *       object of some kind.
 *     * An HTTP-agnostic layer of the API server code base may generate an
 *       error.  It would be nice (but not essential) if these layers did not
 *       need to know about HTTP-specific things like status codes, particularly
 *       since they may not map straightforwardly.  For example, a NotFound
 *       error from the model may not result in a 404 out the API -- it might
 *       just mean that something in the model layer needs to create an object
 *       before using it.
 *     * A library that's not part of the API server code base may generate an
 *       error.  This would include standard library interfaces returning
 *       `std::io::Error` and Hyper returning `hyper::Error`, for examples.
 * * We'd like to take advantage of Rust's built-in error handling control flow
 *   tools, like Results and the '?' operator.
 *
 * Dropshot itself is concerned only with HTTP errors.  We define `HttpError`,
 * which provides a status code, error code (via an Enum), external message (for
 * sending in the response), optional metadata, and an internal message (for the
 * log file or other instrumentation).  The HTTP layers of the request-handling
 * stack may use this struct directly.  **The set of possible error codes here
 * is part of a service's OpenAPI contract, as is the schema for any metadata.**
 * By the time an error bubbles up to the top of the request handling stack, it
 * must be an HttpError.
 *
 * For the HTTP-agnostic layers of an API server (i.e., consumers of Dropshot),
 * we recommend a separate enum to represent their errors in an HTTP-agnostic
 * way.  Consumers can provide a `From` implementation that converts these
 * errors into HttpErrors.
 */

use hyper::error::Error as HyperError;
use serde::Deserialize;
use serde::Serialize;
use serde_json::error::Error as SerdeError;

/**
 * HttpError represents an error generated as part of handling an API
 * request.  When these bubble up to the top of the request handling stack
 * (which is most of the time that they're generated), these are turned into an
 * HTTP response, which includes:
 *
 *   * a status code, which is likely either 400-level (indicating a client
 *     error, like bad input) or 500-level (indicating a server error).
 *   * a structured (JSON) body, which includes:
 *       * a string error code, which identifies the underlying error condition
 *         so that clients can potentially make programmatic decisions based on
 *         the error type
 *       * a string error message, which is the human-readable summary of the
 *         issue, intended to make sense for API users (i.e., not API server
 *         developers)
 *       * optionally: additional metadata describing the issue.  For a
 *         validation error, this could include information about which
 *         parameter was invalid and why.  This should conform to a schema
 *         associated with the error code.
 *
 * It's easy to go overboard with the error codes and metadata.  Generally, we
 * should avoid creating specific codes and metadata unless there's a good
 * reason for a client to care.
 *
 * Besides that, HttpErrors also have an internal error message, which may
 * differ from the error message that gets reported to users.  For example, if
 * the request fails because an internal database is unreachable, the client may
 * just see "internal error", while the server log would include more details
 * like "failed to acquire connection to database at 10.1.2.3".
 */
#[derive(Debug)]
pub struct HttpError {
    /*
     * TODO-polish add string error code and coverage in the test suite
     * TODO-polish add cause chain for a complete log message?
     */
    /** HTTP status code for this error */
    pub status_code: http::StatusCode,
    /** Error message to be sent to API client for this error */
    pub external_message: String,
    /** Error message recorded in the log for this error */
    pub internal_message: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct HttpErrorResponseBody {
    pub message: String,
    pub request_id: String,
}

impl From<SerdeError> for HttpError {
    fn from(error: SerdeError) -> Self {
        /*
         * TODO-polish it would really be much better to annotate this with
         * context about what we were parsing.
         */
        HttpError::for_bad_request(format!("invalid input: {}", error))
    }
}

impl From<HyperError> for HttpError {
    fn from(error: HyperError) -> Self {
        /*
         * TODO-correctness dig deeper into the various cases to make sure this
         * is a valid way to represent it.
         */
        HttpError::for_bad_request(format!(
            "error processing request: {}",
            error
        ))
    }
}

impl From<http::Error> for HttpError {
    fn from(error: http::Error) -> Self {
        /*
         * TODO-correctness dig deeper into the various cases to make sure this
         * is a valid way to represent it.
         */
        HttpError::for_bad_request(format!(
            "error processing request: {}",
            error
        ))
    }
}

impl HttpError {
    pub fn for_bad_request(message: String) -> Self {
        HttpError::for_client_error(http::StatusCode::BAD_REQUEST, message)
    }

    pub fn for_status(code: http::StatusCode) -> Self {
        /* TODO-polish This should probably be our own message. */
        let message = code.canonical_reason().unwrap().to_string();
        HttpError::for_client_error(code, message)
    }

    pub fn for_client_error(code: http::StatusCode, message: String) -> Self {
        assert!(code.is_client_error());
        HttpError {
            status_code: code,
            internal_message: message.clone(),
            external_message: message.clone(),
        }
    }

    pub fn for_internal_error(message_internal: String) -> Self {
        let code = http::StatusCode::INTERNAL_SERVER_ERROR;
        HttpError {
            status_code: code,
            external_message: code.canonical_reason().unwrap().to_string(),
            internal_message: message_internal.clone(),
        }
    }

    pub fn into_response(
        self,
        request_id: &str,
    ) -> hyper::Response<hyper::Body> {
        /*
         * TODO-hardening: consider handling the operational errors that the
         * Serde serialization fails or the response construction fails.  In
         * those cases, we should probably try to report this as a serious
         * problem (e.g., to the log) and send back a 500-level response.  (Of
         * course, that could fail in the same way, but it's less likely because
         * there's only one possible set of input and we can test it.  We'll
         * probably have to use unwrap() there and make sure we've tested that
         * code at least once!)
         */
        hyper::Response::builder()
            .status(self.status_code)
            .header(
                http::header::CONTENT_TYPE,
                super::http_util::CONTENT_TYPE_JSON,
            )
            .header(super::http_util::HEADER_REQUEST_ID, request_id)
            .body(
                serde_json::to_string_pretty(&HttpErrorResponseBody {
                    message: self.external_message,
                    request_id: request_id.to_string(),
                })
                .unwrap()
                .into(),
            )
            .unwrap()
    }
}