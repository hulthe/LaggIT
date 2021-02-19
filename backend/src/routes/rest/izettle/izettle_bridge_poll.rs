use diesel::result::Error;
use diesel::{ExpressionMethods, QueryDsl, QueryResult};
use rocket::{get, State};
use rocket_contrib::json::Json;
use serde::Serialize;
use crate::database::DatabasePool;
use crate::diesel::RunQueryDsl;
use crate::models::izettle_transaction::IZettleTransactionPartial;
use crate::schema::tables::izettle_transaction::dsl::izettle_transaction;
use crate::util::StatusJson;

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum BridgePollResult {
    PendingPayment(IZettleTransactionPartial),
    NoPendingTransaction,
}

#[get("/izettle/bridge/poll")]
pub async fn poll_for_transaction(
    db_pool: State<'_, DatabasePool>,
) -> Result<Json<BridgePollResult>, StatusJson> {
    let connection = db_pool.inner().get()?;

    let transaction_res: QueryResult<IZettleTransactionPartial> = {
        use crate::schema::tables::izettle_transaction::dsl::{amount, id, time};

        izettle_transaction
            .order_by(time.asc())
            .select((id, amount))
            .first(&connection)
    };

    if let Err(Error::NotFound) = transaction_res {
        return Ok(Json(BridgePollResult::NoPendingTransaction));
    }

    // Potential optimization: This function could sleep for up
    // to a few seconds if there is no pending transaction.
    // This way the latency between the server and the bridge would be lower.
    Ok(Json(BridgePollResult::PendingPayment(transaction_res?)))
}
