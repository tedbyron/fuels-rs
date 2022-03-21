pub mod provider;
pub mod util;
pub mod wallet;

use async_trait::async_trait;
use fuel_crypto::Signature;
use fuel_tx::{Address, Transaction};
use std::error::Error;

/// A wallet instantiated with a locally stored private key
pub type LocalWallet = wallet::Wallet;

/// Trait for signing transactions and messages
///
/// Implement this trait to support different signing modes, e.g. Ledger, hosted etc.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Signer: std::fmt::Debug + Send + Sync {
    type Error: Error + Send + Sync;
    /// Signs the hash of the provided message
    async fn sign_message<S: Send + Sync + AsRef<[u8]>>(
        &self,
        message: S,
    ) -> Result<Signature, Self::Error>;

    /// Signs the transaction
    async fn sign_transaction(&self, message: &mut Transaction) -> Result<Signature, Self::Error>;

    /// Returns the signer's Fuel Address
    fn address(&self) -> Address;
}

#[cfg(test)]
mod tests {
    use crate::util::test_helpers::{setup_address_and_coins, setup_test_provider};
    use fuel_crypto::{Message, SecretKey};
    use fuel_tx::{AssetId, Bytes32, Input, Output, UtxoId};
    use rand::{rngs::StdRng, RngCore, SeedableRng};
    use std::str::FromStr;

    use super::*;

    #[tokio::test]
    async fn sign_and_verify() {
        let mut rng = StdRng::seed_from_u64(2322u64);
        let mut secret_seed = [0u8; 32];
        rng.fill_bytes(&mut secret_seed);

        let secret = unsafe { SecretKey::from_bytes_unchecked(secret_seed) };

        let (provider, _) = setup_test_provider(vec![]).await;
        let wallet = LocalWallet::new_from_private_key(secret, provider).unwrap();

        let message = Message::new("my message");

        let signature = wallet.sign_message(message).await.unwrap();

        // TODO(oleksii): impl FromStr for fuel_crypto::Signature
        // Check if signature is what we expect it to be
        // assert_eq!(signature.as_ref(), Signature::from_str("0x8eeb238db1adea4152644f1cd827b552dfa9ab3f4939718bb45ca476d167c6512a656f4d4c7356bfb9561b14448c230c6e7e4bd781df5ee9e5999faa6495163d").unwrap().compact);

        // Recover address that signed the message
        let recovered_address = signature.recover(&message).unwrap();

        assert_eq!(wallet.address.as_ref(), recovered_address.as_ref());

        // Verify signature
        signature.verify(&recovered_address, &message).unwrap();
    }

    #[tokio::test]
    async fn sign_tx_and_verify() {
        let secret =
            SecretKey::from_str("5f70feeff1f229e4a95e1056e8b4d80d0b24b565674860cc213bdb07127ce1b1")
                .unwrap();

        let (provider, _) = setup_test_provider(vec![]).await;
        let wallet = LocalWallet::new_from_private_key(secret, provider).unwrap();

        let input_coin = Input::coin(
            UtxoId::new(Bytes32::zeroed(), 0),
            Address::from_str("0xf1e92c42b90934aa6372e30bc568a326f6e66a1a0288595e6e3fbd392a4f3e6e")
                .unwrap(),
            10000000,
            AssetId::from([0u8; 32]),
            0,
            0,
            vec![],
            vec![],
        );

        let output_coin = Output::coin(
            Address::from_str("0xc7862855b418ba8f58878db434b21053a61a2025209889cc115989e8040ff077")
                .unwrap(),
            1,
            AssetId::from([0u8; 32]),
        );

        let mut tx = Transaction::script(
            0,
            1000000,
            0,
            0,
            hex::decode("24400000").unwrap(),
            vec![],
            vec![input_coin],
            vec![output_coin],
            vec![],
        );

        let signature = wallet.sign_transaction(&mut tx).await.unwrap();
        let message = Message::new(tx.id());

        // TODO(oleksii): impl FromStr for fuel_crypto::Signature
        // Check if signature is what we expect it to be
        // assert_eq!(signature.compact, Signature::from_str("0xa1287a24af13fc102cb9e60988b558d5575d7870032f64bafcc2deda2c99125fb25eca55a29a169de156cb30700965e2b26278fcc7ad375bc720440ea50ba3cb").unwrap().compact);

        // Recover address that signed the transaction
        let recovered_address = signature.recover(&message).unwrap();

        assert_eq!(wallet.address.as_ref(), recovered_address.as_ref());

        // Verify signature
        signature.verify(&recovered_address, &message).unwrap();
    }

    #[tokio::test]
    async fn send_transaction() {
        // Setup two sets of coins, one for each wallet, each containing 1 coin with 1 amount.
        let (pk_1, mut coins_1) = setup_address_and_coins(1, 1);
        let (pk_2, coins_2) = setup_address_and_coins(1, 1);

        coins_1.extend(coins_2);

        // Setup a provider and node with both set of coins
        let (provider, _) = setup_test_provider(coins_1).await;

        let wallet_1 = LocalWallet::new_from_private_key(pk_1, provider.clone()).unwrap();
        let wallet_2 = LocalWallet::new_from_private_key(pk_2, provider).unwrap();

        let wallet_1_initial_coins = wallet_1.get_coins().await.unwrap();
        let wallet_2_initial_coins = wallet_2.get_coins().await.unwrap();

        // Check initial wallet state
        assert_eq!(wallet_1_initial_coins.len(), 1);
        assert_eq!(wallet_2_initial_coins.len(), 1);

        // Transfer 1 from wallet 1 to wallet 2
        let _receipts = wallet_1
            .transfer(&wallet_2.address(), 1, Default::default())
            .await
            .unwrap();

        // Currently ignoring the effect on wallet 1, as coins aren't being marked as spent for now
        let _wallet_1_final_coins = wallet_1.get_coins().await.unwrap();
        let wallet_2_final_coins = wallet_2.get_coins().await.unwrap();

        // Check that wallet two now has two coins
        assert_eq!(wallet_2_final_coins.len(), 2);

        // Transferring more than balance should fail
        let result = wallet_1
            .transfer(&wallet_2.address(), 2, Default::default())
            .await;

        assert!(result.is_err());
        let wallet_2_coins = wallet_2.get_coins().await.unwrap();
        assert_eq!(wallet_2_coins.len(), 2); // Not changed
    }

    #[tokio::test]
    async fn transfer_coins_with_change() {
        // Setup two sets of coins, one for each wallet, each containing 1 coin with 5 amounts each.
        let (pk_1, mut coins_1) = setup_address_and_coins(1, 5);
        let (pk_2, coins_2) = setup_address_and_coins(1, 5);

        coins_1.extend(coins_2);

        let (provider, _) = setup_test_provider(coins_1).await;

        let wallet_1 = LocalWallet::new_from_private_key(pk_1, provider.clone()).unwrap();
        let wallet_2 = LocalWallet::new_from_private_key(pk_2, provider).unwrap();

        let wallet_1_initial_coins = wallet_1.get_coins().await.unwrap();
        let wallet_2_initial_coins = wallet_2.get_coins().await.unwrap();

        assert_eq!(wallet_1_initial_coins.len(), 1);
        assert_eq!(wallet_2_initial_coins.len(), 1);

        // Transfer 2 from wallet 1 to wallet 2.
        let _receipts = wallet_1
            .transfer(&wallet_2.address(), 2, Default::default())
            .await
            .unwrap();

        let wallet_1_final_coins = wallet_1.get_coins().await.unwrap();

        // Assert that we've sent 2 from wallet 1, resulting in an amount of 3 in wallet 1.
        let resulting_amount = wallet_1_final_coins.first().unwrap();
        assert_eq!(resulting_amount.amount.0, 3);

        let wallet_2_final_coins = wallet_2.get_coins().await.unwrap();
        assert_eq!(wallet_2_final_coins.len(), 2);

        // Check that wallet 2's amount is 7:
        // 5 initial + 2 that was sent to it.
        let total_amount: u64 = wallet_2_final_coins.iter().map(|c| c.amount.0).sum();
        assert_eq!(total_amount, 7);
    }
}
