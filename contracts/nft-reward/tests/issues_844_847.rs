#![cfg(test)]

use nft_reward::{
    CollectionMetadata, NftCore, NftErrorCode, NftMetadata, NftReward, NftRewardClient,
};
use soroban_sdk::{
    IntoVal, Symbol, Val,
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Map, String,
};

fn setup_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1000);
    env
}

fn default_collection_metadata(env: &Env) -> CollectionMetadata {
    CollectionMetadata {
        name: String::from_str(env, "Hunty Rewards"),
        description: String::from_str(env, "Reward NFTs for completed hunts"),
        total_supply: 0,
        creator: None,
    }
}

fn create_metadata(env: &Env, title: &str, uri: &str) -> NftMetadata {
    NftMetadata {
        title: String::from_str(env, title),
        description: String::from_str(env, "Desc"),
        image_uri: String::from_str(env, uri),
        hunt_title: String::from_str(env, title),
        rarity: 0,
        tier: 0,
        creator: None,
        royalty_bps: None,
        extensions: Map::new(env),
    }
}

fn setup_contract(env: &Env, max_supply: Option<u64>) -> (Address, NftRewardClient<'_>, Address, Address) {
    let contract_id = env.register_contract(None, NftReward);
    let client = NftRewardClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let minter = Address::generate(env);
    client.initialize(&admin, &minter, &max_supply, &default_collection_metadata(env));
    (contract_id, client, admin, minter)
}

#[test]
fn test_admin_update_image_uris_pagination_and_idempotence() {
    let env = setup_env();
    let (_contract_id, client, admin, minter) = setup_contract(&env, None);
    let owner = Address::generate(&env);

    let nft1 = client.mint_reward_nft(
        &minter,
        &1,
        &owner,
        &create_metadata(&env, "n1", "ipfs://old/a"),
    );
    let nft2 = client.mint_reward_nft(
        &minter,
        &2,
        &owner,
        &create_metadata(&env, "n2", "ipfs://old/b"),
    );
    let nft3 = client.mint_reward_nft(
        &minter,
        &3,
        &owner,
        &create_metadata(&env, "n3", "ipfs://old/c"),
    );

    let old_prefix = String::from_str(&env, "ipfs://old/");
    let new_prefix = String::from_str(&env, "ipfs://new/");

    assert_eq!(
        client.admin_update_image_uris(&admin, &old_prefix, &new_prefix, &0, &2),
        (2, 2)
    );

    assert_eq!(
        client.get_nft(&nft1).unwrap().metadata.image_uri,
        String::from_str(&env, "ipfs://new/a")
    );
    assert_eq!(
        client.get_nft(&nft2).unwrap().metadata.image_uri,
        String::from_str(&env, "ipfs://new/b")
    );
    assert_eq!(
        client.get_nft(&nft3).unwrap().metadata.image_uri,
        String::from_str(&env, "ipfs://old/c")
    );

    assert_eq!(
        client.admin_update_image_uris(&admin, &old_prefix, &new_prefix, &0, &2),
        (0, 2)
    );

    assert_eq!(
        client.admin_update_image_uris(&admin, &old_prefix, &new_prefix, &2, &2),
        (1, 3)
    );

    assert_eq!(
        client.get_nft(&nft3).unwrap().metadata.image_uri,
        String::from_str(&env, "ipfs://new/c")
    );
}

#[test]
fn test_admin_update_image_uris_rejects_oversized_prefix() {
    let env = setup_env();
    let (_contract_id, client, admin, minter) = setup_contract(&env, None);
    let owner = Address::generate(&env);
    let _ = client.mint_reward_nft(
        &minter,
        &1,
        &owner,
        &create_metadata(&env, "n1", "ipfs://old/a"),
    );

    let oversized = String::from_str(&env, &"x".repeat(513));
    let new_prefix = String::from_str(&env, "ipfs://new/");

    assert_eq!(
        client.try_admin_update_image_uris(&admin, &oversized, &new_prefix, &0, &10),
        Err(Ok(NftErrorCode::InvalidMetadata))
    );
}

#[test]
fn test_total_supply_tracks_live_nfts_and_ids_are_not_reused() {
    let env = setup_env();
    let (_contract_id, client, _admin, minter) = setup_contract(&env, Some(2));
    let owner = Address::generate(&env);

    let id1 = client.mint_reward_nft(
        &minter,
        &1,
        &owner,
        &create_metadata(&env, "n1", "ipfs://1"),
    );
    let id2 = client.mint_reward_nft(
        &minter,
        &2,
        &owner,
        &create_metadata(&env, "n2", "ipfs://2"),
    );
    assert_eq!((id1, id2), (1, 2));
    assert_eq!(client.total_supply(), 2);

    client.burn_nft(&id1, &owner);
    assert_eq!(client.total_supply(), 1);

    let id3 = client.mint_reward_nft(
        &minter,
        &3,
        &owner,
        &create_metadata(&env, "n3", "ipfs://3"),
    );
    assert_eq!(id3, 3);
    assert_eq!(client.total_supply(), 2);
}

#[test]
fn test_burn_locked_nft_returns_error_and_preserves_state() {
    let env = setup_env();
    let (contract_id, client, _admin, minter) = setup_contract(&env, None);
    let owner = Address::generate(&env);

    let nft_id = client.mint_reward_nft(
        &minter,
        &1,
        &owner,
        &create_metadata(&env, "locked", "ipfs://locked"),
    );

    let nft = client.get_nft(&nft_id).unwrap();
    let locked_core = NftCore {
        nft_id: nft.nft_id,
        hunt_id: nft.hunt_id,
        owner: nft.owner,
        completion_player: nft.completion_player,
        transferable: nft.transferable,
        minted_at: nft.minted_at,
        locked: true,
    };
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&(symbol_short!("NC"), nft_id), &locked_core);
    });

    assert_eq!(
        client.try_burn_nft(&nft_id, &owner),
        Err(Ok(NftErrorCode::NftLocked))
    );
    assert!(client.get_nft(&nft_id).is_some());
    assert_eq!(client.get_player_nfts(&owner, &0, &10).len(), 1);
}

#[test]
fn test_soulbound_nft_can_still_be_burned_by_owner() {
    let env = setup_env();
    let (_contract_id, client, _admin, minter) = setup_contract(&env, None);
    let owner = Address::generate(&env);

    let mut metadata = Map::<Symbol, Val>::new(&env);
    metadata.set(
        Symbol::new(&env, "title"),
        String::from_str(&env, "soulbound").into_val(&env),
    );
    metadata.set(
        Symbol::new(&env, "description"),
        String::from_str(&env, "Desc").into_val(&env),
    );
    metadata.set(
        Symbol::new(&env, "image_uri"),
        String::from_str(&env, "ipfs://soulbound").into_val(&env),
    );
    metadata.set(
        Symbol::new(&env, "transferable"),
        false.into_val(&env),
    );

    let nft_id = client.mint_reward_nft_from_map(&minter, &1, &owner, &metadata);

    let nft = client.get_nft(&nft_id).unwrap();
    assert!(!nft.transferable);

    client.burn_nft(&nft_id, &owner);
    assert!(client.get_nft(&nft_id).is_none());
}
