// Bindings to database views aren't automatically generated by diesel.
// This file has to be updated manually.

table! {
    events_with_signups (id) {
        id -> Int4,
        title -> Text,
        background -> Text,
        location -> Text,
        start_time -> Timestamp,
        end_time -> Timestamp,
        price -> Int4,
        published -> Bool,
        signups -> Int8,
    }
}

table! {
    inventory_stock (name) {
        id -> Int4,
        name -> Text,
        stock -> Int4,
    }
}

table! {
    transactions_joined (id) {
        id -> Int4,
        amount -> Int4,
        description -> Nullable<Text>,
        time -> Timestamp,

        bundle_id -> Int4,
        bundle_price -> Nullable<Int4>,
        change -> Int4,

        item_id -> Int4,
    }
}
