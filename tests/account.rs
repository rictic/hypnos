use hypnos::data::Account;

#[test]
fn test_overdrafted_true() {
    let account = Account { user: String::new(), images: 0, credit: -1, total_cost: 0 };
    assert!(account.overdrafted());
}

#[test]
fn test_overdrafted_false() {
    let account = Account { user: String::new(), images: 0, credit: 1000, total_cost: 0 };
    assert!(!account.overdrafted());
}
