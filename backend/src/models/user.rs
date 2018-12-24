use crate::database::{DatabaseConn, DatabasePool};
use crate::schema::tables::users;
use diesel::prelude::*;
use orion::errors::UnknownCryptoError;
use orion::pwhash::{hash_password, Password};
use rocket::http::{Cookie, Status};
use rocket::request::{self, FromRequest, Request};
use rocket::{Outcome, State};
use serde_json;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

pub const PWHASH_ITERATIONS: usize = 10000;
/// This struct defines a user object
///
/// It's used as a request guard: all routes with a User parameter will return
/// 401 UNAUTHORIZED if the client has not previously authenticated.
#[derive(Queryable, Insertable, Serialize, Deserialize, Debug)]
pub struct User {
    pub name: String,
    pub display_name: Option<String>,
    pub salted_pass: String,
    pub hash_iterations: i32,
}

static SESSION_COOKIE_KEY: &str = "session";
#[derive(Serialize, Deserialize)]
pub struct Session {
    pub username: String,
    pub last_seen: u64,
}

pub fn make_user_session<'a>(user: &User) -> Cookie<'a> {
    let session = Session {
        username: user.name.clone(),
        last_seen: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };
    let serialized = serde_json::to_string(&session).expect("Could not serialize session");
    Cookie::new(SESSION_COOKIE_KEY, serialized)
}

fn get_user(user_name: String, connection: &DatabaseConn) -> Option<User> {
    use crate::schema::tables::users::dsl::*;
    users.find(user_name).first(connection).ok()
}

impl<'a, 'r> FromRequest<'a, 'r> for User {
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> request::Outcome<User, ()> {
        let mut cookies = request.cookies();
        let db_pool: State<DatabasePool> = request.guard()?;

        let session: Option<Session> = cookies
            .get_private(SESSION_COOKIE_KEY)
            .and_then(|user| serde_json::from_str(user.value()).ok());

        if session.is_none() {
            return Outcome::Forward(());
        }

        let session = session.unwrap();

        if let Some(session_lifetime) = env::var("SESSION_LIFETIME")
            .ok()
            .and_then(|lifetime| lifetime.parse().ok())
        {
            let unix_time: u64 = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            if session.last_seen > unix_time {
                eprintln!(
                    "Client `last_seen` variable is set as the future. \
                     Is the server system time misconfigured?"
                );
                return Outcome::Failure((Status::InternalServerError, ()));
            } else if unix_time - session.last_seen > session_lifetime {
                return Outcome::Forward(());
            }
        }

        if let Ok(connection) = db_pool.inner().get() {
            if let Some(user) = get_user(session.username, &connection) {
                Outcome::Success(user)
            } else {
                Outcome::Forward(())
            }
        } else {
            Outcome::Failure((Status::InternalServerError, ()))
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Credentials {
    pub name: String,
    pub pass: String,
}

pub fn generate_salted_hash<T: AsRef<[u8]>>(password: T) -> Result<String, UnknownCryptoError> {
    hash_password(&Password::from_slice(password.as_ref()), PWHASH_ITERATIONS)
        .map(|pwhash| hex::encode(&pwhash.unprotected_as_bytes()))
}
