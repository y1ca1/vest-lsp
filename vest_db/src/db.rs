//! Salsa database trait and implementation.

/// The Salsa database trait for Vest.
#[salsa::db]
pub trait Db: salsa::Database {}

/// Default database implementation.
#[salsa::db]
#[derive(Default)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

impl Database {
    pub fn new() -> Self {
        Self::default()
    }
}

#[salsa::db]
impl salsa::Database for Database {}

#[salsa::db]
impl Db for Database {}
