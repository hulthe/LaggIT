use crate::app::StateReady;
use crate::components::parsed_input::{ParsedInput, ParsedInputMsg};
use crate::generated::css_classes::C;
use crate::strings;
use crate::util::simple_ev;
use seed::app::cmds::timeout;
use seed::prelude::*;
use seed::*;
use std::collections::HashMap;
use std::ops::Deref;
use strecklistan_api::{
    book_account::BookAccountId,
    currency::Currency,
    inventory::{InventoryBundleId, InventoryItemId, InventoryItemStock as InventoryItem},
    transaction::{NewTransaction, TransactionBundle, TransactionId},
};

const SUCCESS_ANIMATION_TIMEOUT: u32 = 1240;

#[derive(Clone, Debug)]
pub enum CheckoutMsg {
    ConfirmPurchase,
    PurchaseSent {
        transaction_id: TransactionId,
    },

    TotalInputMsg(ParsedInputMsg),
    AddItem {
        item_id: InventoryItemId,
        amount: i32,
    },
    AddBundle {
        bundle_id: InventoryBundleId,
        amount: i32,
    },
    SetBundleChange {
        bundle_index: usize,
        change: i32,
    },

    AnimationFinished,
}

#[derive(Clone)]
pub struct Checkout {
    transaction: NewTransaction,
    transaction_total_input: ParsedInput<Currency>,
    override_transaction_total: bool,
    show_success_animation: bool,
    pub confirm_button_message: Option<&'static str>,
}

impl Checkout {
    pub fn new(global: &StateReady) -> Self {
        Checkout {
            transaction: NewTransaction {
                description: Some(strings::TRANSACTION_SALE.to_string()),
                bundles: vec![],
                amount: 0.into(),
                debited_account: global.master_accounts.bank_account_id,
                credited_account: global.master_accounts.sales_account_id,
            },
            transaction_total_input: ParsedInput::new("0")
                .with_error_message(strings::INVALID_MONEY_MESSAGE_SHORT)
                .with_input_kind("text"),
            override_transaction_total: false,
            confirm_button_message: None,
            show_success_animation: false,
        }
    }

    pub fn update(
        &mut self,
        msg: CheckoutMsg,
        global: &mut StateReady,
        orders: &mut impl Orders<CheckoutMsg>,
    ) {
        match msg {
            CheckoutMsg::ConfirmPurchase => {
                self.show_success_animation = true;

                orders.perform_cmd(timeout(SUCCESS_ANIMATION_TIMEOUT, move || {
                    CheckoutMsg::AnimationFinished
                }));
            }
            CheckoutMsg::AnimationFinished => {
                self.show_success_animation = false;
                self.remove_cleared_items();
                global.request_in_progress = true;
                let msg = self.transaction.clone();

                orders.perform_cmd(async move {
                    let result = async {
                        Request::new("/api/transaction")
                            .method(Method::Post)
                            .json(&msg)?
                            .fetch()
                            .await?
                            .json()
                            .await
                    }
                    .await;
                    match result {
                        Ok(transaction_id) => Some(CheckoutMsg::PurchaseSent { transaction_id }),
                        Err(e) => {
                            error!("Failed to post purchase", e);
                            None
                        }
                    }
                });
            }
            CheckoutMsg::PurchaseSent { transaction_id } => {
                global.request_in_progress = false;
                log!("Posted transaction ID: ", transaction_id);
                self.transaction.amount = 0.into();
                self.transaction.bundles = vec![];
                self.transaction.description = Some(strings::TRANSACTION_SALE.into());
            }
            CheckoutMsg::TotalInputMsg(msg) => {
                match &msg {
                    ParsedInputMsg::FocusOut => {
                        if self.transaction_total_input.get_value().is_none() {
                            self.override_transaction_total = false;
                            self.recompute_new_transaction_total();
                        }
                    }
                    ParsedInputMsg::Input(_) => {
                        self.override_transaction_total = true;
                    }
                    _ => {}
                }
                self.transaction_total_input.update(msg);

                if self.override_transaction_total {
                    let new_total = self.transaction_total_input.get_value().copied();
                    log!(format!("new transaction total: {:?}", new_total));
                    self.transaction.amount = new_total.unwrap_or(0.into());
                }
            }
            CheckoutMsg::AddItem { item_id, amount } => {
                let item = global
                    .inventory
                    .get(&item_id)
                    .unwrap_or_else(|| panic!("No inventory item with that id exists"))
                    .clone();

                let mut item_ids = HashMap::new();
                item_ids.insert(item.id, 1);

                let bundle = TransactionBundle {
                    description: None,
                    // TODO: Handle case where price is null
                    price: Some(item.price.unwrap_or(0).into()),
                    change: -amount,
                    item_ids,
                };

                if let Some(b) =
                    self.transaction.bundles.iter_mut().find(|b| {
                        b.item_ids == bundle.item_ids && b.description == bundle.description
                    })
                {
                    b.change -= amount;
                } else {
                    log!("Pushing bundle", bundle);
                    self.transaction.bundles.push(bundle);
                }
            }
            CheckoutMsg::AddBundle { bundle_id, amount } => {
                let bundle = global
                    .bundles
                    .get(&bundle_id)
                    .unwrap_or_else(|| panic!("No inventory bundle with that id exists"))
                    .clone();

                let mut item_ids = HashMap::new();
                for &id in bundle.item_ids.iter() {
                    *item_ids.entry(id).or_default() += 1;
                }

                let bundle = TransactionBundle {
                    description: Some(bundle.name.clone()),
                    price: Some(bundle.price),
                    change: -amount,
                    item_ids,
                };

                if let Some(b) =
                    self.transaction.bundles.iter_mut().find(|b| {
                        b.item_ids == bundle.item_ids && b.description == bundle.description
                    })
                {
                    b.change -= amount;
                } else {
                    log!("Pushing bundle", bundle);
                    self.transaction.bundles.push(bundle);
                }
            }
            CheckoutMsg::SetBundleChange {
                bundle_index,
                change,
            } => {
                let bundle = &mut self.transaction.bundles[bundle_index];
                if !self.override_transaction_total {
                    let diff = bundle.change - change;
                    self.transaction.amount +=
                        (bundle.price.map(|p| p.into()).unwrap_or(0i32) * diff).into();
                }
                bundle.change = change;
            }
        }

        self.recompute_new_transaction_total();
    }

    fn recompute_new_transaction_total(&mut self) {
        if !self.override_transaction_total {
            self.transaction.amount = self
                .transaction
                .bundles
                .iter()
                .map(|bundle| -bundle.change * bundle.price.map(|p| p.into()).unwrap_or(0i32))
                .sum::<i32>()
                .into();
            self.transaction_total_input
                .set_value(self.transaction.amount);
        }
    }

    pub fn transaction(&self) -> &NewTransaction {
        &self.transaction
    }

    pub fn set_debited(&mut self, acc_id: BookAccountId) {
        self.transaction.debited_account = acc_id;
    }

    pub fn remove_cleared_items(&mut self) {
        self.transaction.bundles.retain(|bundle| bundle.change != 0);
    }

    pub fn view(&self, global: &StateReady) -> Node<CheckoutMsg> {
        div![
            C![C.new_transaction_view],
            self.transaction
                .bundles
                .iter()
                .enumerate()
                .rev() // display newest bundle first
                .map(|(bundle_index, bundle)| {
                    let mut items = bundle.item_ids.keys().map(|id| &global.inventory[id]);

                    // TODO: Properly display more complicated bundles

                    let (item_name, item_price) = match items.next().map(|rc| rc.deref()) {
                        None => (None, 0),
                        Some(InventoryItem { name, price, .. }) => {
                            (Some(name.as_str()), price.unwrap_or(0))
                        }
                    };

                    let name = bundle
                        .description
                        .as_ref()
                        .map(|s| s.as_str())
                        .unwrap_or(item_name.unwrap_or("[NAMN SAKNAS]"));
                    let price = bundle.price.unwrap_or(item_price.into());

                    p![
                        if bundle.change == 0 {
                            C![C.line_through, C.transaction_entry]
                        } else {
                            C![C.transaction_entry]
                        },
                        input![
                            C![C.new_transaction_bundle_amount_field, C.border_on_focus],
                            attrs! { At::Value => -bundle.change },
                            attrs! { At::Type => "number" },
                            input_ev(Ev::Input, move |input| {
                                CheckoutMsg::SetBundleChange {
                                    bundle_index,
                                    change: -input.parse().unwrap_or(0),
                                }
                            }),
                        ],
                        span![C![C.transaction_entry_item_name], format!("x {}", name),],
                        span![C![C.transaction_entry_item_price], format!("{}:-", price),],
                    ]
                })
                .collect::<Vec<_>>(),
            p![span!["Totalt: "], {
                let amount = self.transaction.amount.to_string();
                let _len = (amount.len() as f32) / 2.0 + 0.5;
                let color = if self.override_transaction_total {
                    "color: #762;"
                } else {
                    "color: black;"
                };
                let attrs = attrs! {
                    At::Style => color,
                    At::Class => C.new_transaction_total_field,
                    At::Class => C.border_on_focus,
                };
                self.transaction_total_input
                    .view(attrs)
                    .map_msg(CheckoutMsg::TotalInputMsg)
            }],
            if !global.request_in_progress {
                button![
                    C![C.wide_button, C.border_on_focus],
                    simple_ev(Ev::Click, CheckoutMsg::ConfirmPurchase),
                    "Slutför Köp",
                ]
            } else {
                button![
                    C![C.wide_button, C.border_on_focus],
                    div![
                        C![C.lds_ripple],
                        style! {
                            St::Position => "absolute",
                            St::MarginTop => "-20px",
                        },
                        div![],
                        div![],
                    ],
                    attrs! { At::Disabled => true },
                    "Slutför Köp",
                ]
            },
            if let Some(message) = &self.confirm_button_message {
                div![C![C.wide_button_message], message]
            } else {
                empty![]
            },
            if self.show_success_animation {
                div![
                    class![C.success_gif_container],
                    img![
                        class![C.success_gif],
                        attrs! {At::Src => "/static/success_animation.gif"},
                    ]
                ]
            } else {
                empty![]
            },
        ]
    }
}
