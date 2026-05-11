use anchor_lang;
use anchor_lang::prelude::Pubkey as APubkey;
use anchor_lang::{declare_program, AccountDeserialize};
use anchor_litesvm::{AnchorContext, AnchorLiteSVM, Pubkey, Signer, TestHelpers, AssertionHelpers};
use solana_native_token::LAMPORTS_PER_SOL;

declare_program!(time_locked_vault);

const PROGRAM_BYTES: &[u8] = include_bytes!("../../../target/deploy/time_locked_vault.so");

fn ap(p: Pubkey) -> APubkey { APubkey::from(p.to_bytes()) }
fn sys() -> APubkey { APubkey::default() }
fn tok() -> APubkey { APubkey::from(anchor_spl::token::ID.to_bytes()) }

fn program_id() -> Pubkey {
    time_locked_vault::ID.to_bytes().into()
}

fn setup() -> AnchorContext {
    AnchorLiteSVM::build_with_program(program_id(), PROGRAM_BYTES)
}

fn vault_state_pda(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"vault_state", owner.as_ref(), mint.as_ref()],
        &program_id(),
    ).0
}

fn vault_token_pda(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"vault", owner.as_ref(), mint.as_ref()],
        &program_id(),
    ).0
}

fn get_vault_state(ctx: &AnchorContext, key: &Pubkey) -> time_locked_vault::accounts::VaultState {
    let acct = ctx.svm.get_account(key).expect("vault_state not found");
    time_locked_vault::accounts::VaultState::try_deserialize(&mut acct.data.as_ref())
        .expect("deserialize failed")
}

fn warp_to(ctx: &mut AnchorContext, unix_timestamp: i64) {
    let mut clock = ctx.svm.get_sysvar::<solana_program::clock::Clock>();
    clock.unix_timestamp = unix_timestamp;
    ctx.svm.set_sysvar::<solana_program::clock::Clock>(&clock);
}

// Anchor encodes return values as little-endian bytes in a "Program return: <id> <base64>" log line.
fn parse_i64_return(logs: &[String]) -> i64 {
    use base64::{Engine, engine::general_purpose::STANDARD};
    for log in logs {
        if let Some(rest) = log.strip_prefix("Program return: ") {
            let b64 = rest.split_whitespace().nth(1).expect("no base64 in return log");
            let bytes = STANDARD.decode(b64).expect("base64 decode failed");
            let arr: [u8; 8] = bytes.try_into().expect("return value not 8 bytes");
            return i64::from_le_bytes(arr);
        }
    }
    panic!("no Program return log found");
}

const LOCK_DURATION: i64 = 60;
const TOKEN_AMOUNT: u64 = 1_000_000;

// ── initialize ────────────────────────────────────────────────────────────────

#[test]
fn test_initialize() {
    let mut ctx = setup();
    let owner   = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint    = mint_kp.pubkey();
    let vs      = vault_state_pda(&owner.pubkey(), &mint);
    let vta     = vault_token_pda(&owner.pubkey(), &mint);

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    let state = get_vault_state(&ctx, &vs);
    assert_eq!(state.owner.to_bytes(), owner.pubkey().to_bytes());
    assert_eq!(state.mint.to_bytes(), mint.to_bytes());
    assert_eq!(state.lock_duration, LOCK_DURATION);
    assert!(state.unlock_time > 0);
    assert!(state.bump > 0);
    assert!(state.vault_bump > 0);
}

#[test]
fn test_initialize_twice_fails() {
    let mut ctx = setup();
    let owner   = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint    = mint_kp.pubkey();
    let vs      = vault_state_pda(&owner.pubkey(), &mint);
    let vta     = vault_token_pda(&owner.pubkey(), &mint);

    let make_ix = |ctx: &AnchorContext| {
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap()
    };

    ctx.execute_instruction(make_ix(&ctx), &[&owner]).unwrap().assert_success();
    ctx.execute_instruction(make_ix(&ctx), &[&owner]).unwrap().assert_failure();
}

#[test]
fn test_initialize_lock_too_short_fails() {
    let mut ctx = setup();
    let owner   = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint    = mint_kp.pubkey();
    let vs      = vault_state_pda(&owner.pubkey(), &mint);
    let vta     = vault_token_pda(&owner.pubkey(), &mint);

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: 30 })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_failure();
}

// ── deposit ───────────────────────────────────────────────────────────────────

#[test]
fn test_deposit() {
    let mut ctx   = setup();
    let owner     = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp   = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint      = mint_kp.pubkey();
    let owner_ta  = ctx.svm.create_associated_token_account(&mint, &owner).unwrap();
    let vs        = vault_state_pda(&owner.pubkey(), &mint);
    let vta       = vault_token_pda(&owner.pubkey(), &mint);

    ctx.svm.mint_to(&mint, &owner_ta, &owner, TOKEN_AMOUNT).unwrap();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Deposit {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Deposit { amount: TOKEN_AMOUNT })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.svm.assert_token_balance(&vta, TOKEN_AMOUNT);
    ctx.svm.assert_token_balance(&owner_ta, 0);
    assert_eq!(get_vault_state(&ctx, &vs).total_deposited, TOKEN_AMOUNT);
}

#[test]
fn test_deposit_zero_fails() {
    let mut ctx   = setup();
    let owner     = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp   = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint      = mint_kp.pubkey();
    let owner_ta  = ctx.svm.create_associated_token_account(&mint, &owner).unwrap();
    let vs        = vault_state_pda(&owner.pubkey(), &mint);
    let vta       = vault_token_pda(&owner.pubkey(), &mint);

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Deposit {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Deposit { amount: 0 })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_failure();
}

#[test]
fn test_deposit_while_locked_succeeds() {
    let mut ctx   = setup();
    let owner     = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp   = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint      = mint_kp.pubkey();
    let owner_ta  = ctx.svm.create_associated_token_account(&mint, &owner).unwrap();
    let vs        = vault_state_pda(&owner.pubkey(), &mint);
    let vta       = vault_token_pda(&owner.pubkey(), &mint);

    ctx.svm.mint_to(&mint, &owner_ta, &owner, TOKEN_AMOUNT).unwrap();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Deposit {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Deposit { amount: TOKEN_AMOUNT })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();
}

// ── withdraw ──────────────────────────────────────────────────────────────────

#[test]
fn test_withdraw_before_unlock_fails() {
    let mut ctx   = setup();
    let owner     = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp   = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint      = mint_kp.pubkey();
    let owner_ta  = ctx.svm.create_associated_token_account(&mint, &owner).unwrap();
    let vs        = vault_state_pda(&owner.pubkey(), &mint);
    let vta       = vault_token_pda(&owner.pubkey(), &mint);

    ctx.svm.mint_to(&mint, &owner_ta, &owner, TOKEN_AMOUNT).unwrap();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Deposit {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Deposit { amount: TOKEN_AMOUNT })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Withdraw {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Withdraw { amount: TOKEN_AMOUNT })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_failure();
}

#[test]
fn test_withdraw_after_unlock_succeeds() {
    let mut ctx   = setup();
    let owner     = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp   = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint      = mint_kp.pubkey();
    let owner_ta  = ctx.svm.create_associated_token_account(&mint, &owner).unwrap();
    let vs        = vault_state_pda(&owner.pubkey(), &mint);
    let vta       = vault_token_pda(&owner.pubkey(), &mint);

    ctx.svm.mint_to(&mint, &owner_ta, &owner, TOKEN_AMOUNT).unwrap();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Deposit {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Deposit { amount: TOKEN_AMOUNT })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    let unlock_time = get_vault_state(&ctx, &vs).unlock_time;
    warp_to(&mut ctx, unlock_time + 1);

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Withdraw {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Withdraw { amount: TOKEN_AMOUNT })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.svm.assert_token_balance(&vta, 0);
    ctx.svm.assert_token_balance(&owner_ta, TOKEN_AMOUNT);
    assert_eq!(get_vault_state(&ctx, &vs).total_deposited, 0);
}

#[test]
fn test_withdraw_overdraft_fails() {
    let mut ctx   = setup();
    let owner     = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp   = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint      = mint_kp.pubkey();
    let owner_ta  = ctx.svm.create_associated_token_account(&mint, &owner).unwrap();
    let vs        = vault_state_pda(&owner.pubkey(), &mint);
    let vta       = vault_token_pda(&owner.pubkey(), &mint);

    ctx.svm.mint_to(&mint, &owner_ta, &owner, TOKEN_AMOUNT).unwrap();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Deposit {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Deposit { amount: TOKEN_AMOUNT })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    let unlock_time = get_vault_state(&ctx, &vs).unlock_time;
    warp_to(&mut ctx, unlock_time + 1);

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Withdraw {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Withdraw { amount: TOKEN_AMOUNT * 999 })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_failure();
}

#[test]
fn test_withdraw_unauthorized_fails() {
    let mut ctx      = setup();
    let owner        = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let attacker     = ctx.create_funded_account(5 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp      = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint         = mint_kp.pubkey();
    let owner_ta     = ctx.svm.create_associated_token_account(&mint, &owner).unwrap();
    let attacker_ta  = ctx.svm.create_associated_token_account(&mint, &attacker).unwrap();
    let vs           = vault_state_pda(&owner.pubkey(), &mint);
    let vta          = vault_token_pda(&owner.pubkey(), &mint);

    ctx.svm.mint_to(&mint, &owner_ta, &owner, TOKEN_AMOUNT).unwrap();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Deposit {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Deposit { amount: TOKEN_AMOUNT })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    let unlock_time = get_vault_state(&ctx, &vs).unlock_time;
    warp_to(&mut ctx, unlock_time + 1);

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Withdraw {
                owner: ap(attacker.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(attacker_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Withdraw { amount: TOKEN_AMOUNT })
            .instruction().unwrap(),
        &[&attacker],
    ).unwrap().assert_failure();
}

// ── close ─────────────────────────────────────────────────────────────────────

#[test]
fn test_close_before_unlock_fails() {
    let mut ctx   = setup();
    let owner     = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp   = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint      = mint_kp.pubkey();
    let owner_ta  = ctx.svm.create_associated_token_account(&mint, &owner).unwrap();
    let vs        = vault_state_pda(&owner.pubkey(), &mint);
    let vta       = vault_token_pda(&owner.pubkey(), &mint);

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Close {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Close {})
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_failure();
}

#[test]
fn test_close_after_unlock_succeeds() {
    let mut ctx   = setup();
    let owner     = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp   = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint      = mint_kp.pubkey();
    let owner_ta  = ctx.svm.create_associated_token_account(&mint, &owner).unwrap();
    let vs        = vault_state_pda(&owner.pubkey(), &mint);
    let vta       = vault_token_pda(&owner.pubkey(), &mint);

    ctx.svm.mint_to(&mint, &owner_ta, &owner, TOKEN_AMOUNT).unwrap();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Deposit {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(),
            })
            .args(time_locked_vault::client::args::Deposit { amount: TOKEN_AMOUNT })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    let unlock_time = get_vault_state(&ctx, &vs).unlock_time;
    warp_to(&mut ctx, unlock_time + 1);

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Close {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
                owner_token_account: ap(owner_ta), vault_token_account: ap(vta),
                token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Close {})
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    assert!(!ctx.account_exists(&vta));
    assert!(!ctx.account_exists(&vs));
    ctx.svm.assert_token_balance(&owner_ta, TOKEN_AMOUNT);
}

// ── time_remaining ────────────────────────────────────────────────────────────

#[test]
fn test_time_remaining_while_locked() {
    let mut ctx  = setup();
    let owner    = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp  = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint     = mint_kp.pubkey();
    let vs       = vault_state_pda(&owner.pubkey(), &mint);
    let vta      = vault_token_pda(&owner.pubkey(), &mint);

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    let result = ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::TimeRemaining {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
            })
            .args(time_locked_vault::client::args::TimeRemaining {})
            .instruction().unwrap(),
        &[&owner],
    ).unwrap();
    result.assert_success();

    let remaining = parse_i64_return(result.logs());
    assert!(remaining > 0, "expected time remaining > 0 while locked, got {remaining}");
    assert!(remaining <= LOCK_DURATION, "time remaining exceeds lock duration");
}

#[test]
fn test_time_remaining_after_unlock() {
    let mut ctx  = setup();
    let owner    = ctx.create_funded_account(10 * LAMPORTS_PER_SOL).unwrap();
    let mint_kp  = ctx.svm.create_token_mint(&owner, 6).unwrap();
    let mint     = mint_kp.pubkey();
    let vs       = vault_state_pda(&owner.pubkey(), &mint);
    let vta      = vault_token_pda(&owner.pubkey(), &mint);

    ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::Initialize {
                owner: ap(owner.pubkey()), mint: ap(mint), vault_state: ap(vs),
                vault_token_account: ap(vta), token_program: tok(), system_program: sys(),
            })
            .args(time_locked_vault::client::args::Initialize { lock_duration: LOCK_DURATION })
            .instruction().unwrap(),
        &[&owner],
    ).unwrap().assert_success();

    let unlock_time = get_vault_state(&ctx, &vs).unlock_time;
    warp_to(&mut ctx, unlock_time + 1);

    let result = ctx.execute_instruction(
        ctx.program()
            .accounts(time_locked_vault::client::accounts::TimeRemaining {
                owner: ap(owner.pubkey()), vault_state: ap(vs),
            })
            .args(time_locked_vault::client::args::TimeRemaining {})
            .instruction().unwrap(),
        &[&owner],
    ).unwrap();
    result.assert_success();

    let remaining = parse_i64_return(result.logs());
    assert_eq!(remaining, 0, "expected 0 after unlock, got {remaining}");
}
