#[derive(Debug, Clone)]
pub struct Group {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct Account {
    pub id: i64,
    pub group_id: i64,
    pub site: String,
    pub email: String,
    pub region: String,
    pub payment_methods: String,
    pub notes: String,
}
