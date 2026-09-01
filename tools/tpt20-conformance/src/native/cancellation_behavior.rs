use tpt20_rpc::CancellationToken;

#[test]
fn new_token_not_cancelled() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn cancelled_token_is_cancelled() {
    let token = CancellationToken::cancelled();
    assert!(token.is_cancelled());
}

#[test]
fn cancel_propagates_to_clones() {
    let token = CancellationToken::new();
    let clone = token.clone();
    token.cancel();
    assert!(clone.is_cancelled());
    assert!(token.is_cancelled());
}

#[test]
fn default_creates_uncancelled_token() {
    let token = CancellationToken::default();
    assert!(!token.is_cancelled());
}
