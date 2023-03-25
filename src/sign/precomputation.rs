//! Precomputation for one-round signing.

use core::ops::Mul;

use crate::utils::Vec;
use crate::{Error, FrostResult};

use crate::ciphersuite::CipherSuite;

use ark_ec::{CurveGroup, Group};
use ark_ff::{PrimeField, Zero};
use ark_serialize::CanonicalDeserialize;
use ark_serialize::CanonicalSerialize;

use rand::CryptoRng;
use rand::Rng;
use zeroize::Zeroize;

#[derive(Debug, CanonicalSerialize, CanonicalDeserialize, Zeroize)]
pub(crate) struct NoncePair<F: PrimeField>(pub(crate) F, pub(crate) F);

impl<F: PrimeField> Drop for NoncePair<F> {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl<F: PrimeField> NoncePair<F> {
    pub fn new(mut csprng: impl CryptoRng + Rng) -> Self {
        NoncePair(F::rand(&mut csprng), F::rand(&mut csprng))
    }
}

impl<C: CipherSuite> From<NoncePair<<C::G as Group>::ScalarField>> for CommitmentShare<C> {
    fn from(other: NoncePair<<C::G as Group>::ScalarField>) -> Self {
        let x = C::G::generator().mul(other.0);
        let y = C::G::generator().mul(other.1);

        Self {
            hiding: Commitment {
                secret: other.0,
                commit: x,
            },
            binding: Commitment {
                secret: other.1,
                commit: y,
            },
        }
    }
}

/// A pair of a secret and a commitment to it.
#[derive(Clone, Debug, Eq, CanonicalSerialize, CanonicalDeserialize)]
pub(crate) struct Commitment<C: CipherSuite> {
    /// The secret.
    pub(crate) secret: <C::G as Group>::ScalarField,
    /// The commitment.
    pub(crate) commit: <C as CipherSuite>::G,
}

impl<C: CipherSuite> Zeroize for Commitment<C> {
    fn zeroize(&mut self) {
        self.secret.zeroize();
        // We set the commitment to the identity point, as the Group trait
        // does not implement Zeroize.
        // Safely zeroizing of the secret component is what actually matters.
        self.commit = <C as CipherSuite>::G::zero();
    }
}

impl<C: CipherSuite> Drop for Commitment<C> {
    fn drop(&mut self) {
        self.zeroize();
    }
}

/// Test equality in constant-time.
impl<C: CipherSuite> PartialEq for Commitment<C> {
    fn eq(&self, other: &Self) -> bool {
        self.secret.eq(&other.secret) & self.commit.into_affine().eq(&other.commit.into_affine())
    }
}

/// A precomputed commitment share.
#[derive(Clone, Debug, Eq, CanonicalSerialize, CanonicalDeserialize, Zeroize)]
pub struct CommitmentShare<C: CipherSuite> {
    /// The hiding commitment.
    ///
    /// This is \\((d\_{ij}, D\_{ij})\\) in the paper.
    pub(crate) hiding: Commitment<C>,
    /// The binding commitment.
    ///
    /// This is \\((e\_{ij}, E\_{ij})\\) in the paper.
    pub(crate) binding: Commitment<C>,
}

impl<C: CipherSuite> CommitmentShare<C> {
    /// Serialize this `CommitmentShare` to a vector of bytes.
    pub fn to_bytes(&self) -> FrostResult<C, Vec<u8>> {
        let mut bytes = Vec::new();

        self.serialize_compressed(&mut bytes)
            .map_err(|_| Error::SerialisationError)?;

        Ok(bytes)
    }

    /// Attempt to deserialize a `CommitmentShare` from a vector of bytes.
    pub fn from_bytes(bytes: &[u8]) -> FrostResult<C, Self> {
        Self::deserialize_compressed(bytes).map_err(|_| Error::DeserialisationError)
    }
}

impl<C: CipherSuite> Drop for CommitmentShare<C> {
    fn drop(&mut self) {
        self.zeroize();
    }
}

/// Test equality in constant-time.
impl<C: CipherSuite> PartialEq for CommitmentShare<C> {
    fn eq(&self, other: &Self) -> bool {
        self.hiding.eq(&other.hiding) & self.binding.eq(&other.binding)
    }
}

impl<C: CipherSuite> CommitmentShare<C> {
    /// Publish the public commitments in this [`CommitmentShare`].
    pub fn publish(&self) -> (C::G, C::G) {
        (self.hiding.commit, self.binding.commit)
    }
}

/// A secret commitment share list, containing the revealed secrets for the
/// hiding and binding commitments.
#[derive(Clone, Debug, Eq, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct SecretCommitmentShareList<C: CipherSuite> {
    /// The secret commitment shares.
    pub commitments: Vec<CommitmentShare<C>>,
}

impl<C: CipherSuite> SecretCommitmentShareList<C> {
    /// Serialize this `SecretCommitmentShareList` to a vector of bytes.
    pub fn to_bytes(&self) -> FrostResult<C, Vec<u8>> {
        let mut bytes = Vec::new();

        self.serialize_compressed(&mut bytes)
            .map_err(|_| Error::SerialisationError)?;

        Ok(bytes)
    }

    /// Attempt to deserialize a `SecretCommitmentShareList` from a vector of bytes.
    pub fn from_bytes(bytes: &[u8]) -> FrostResult<C, Self> {
        Self::deserialize_compressed(bytes).map_err(|_| Error::DeserialisationError)
    }
}

/// A public commitment share list, containing only the hiding and binding
/// commitments, *not* their committed-to secret values.
///
/// This should be published somewhere before the signing protocol takes place
/// for the other signing participants to obtain.
#[derive(Debug, Eq, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct PublicCommitmentShareList<C: CipherSuite> {
    /// The participant's index.
    pub participant_index: u32,
    /// The published commitments.
    pub commitments: Vec<(C::G, C::G)>,
}

impl<C: CipherSuite> PublicCommitmentShareList<C> {
    /// Serialize this `PublicCommitmentShareList` to a vector of bytes.
    pub fn to_bytes(&self) -> FrostResult<C, Vec<u8>> {
        let mut bytes = Vec::new();

        self.serialize_compressed(&mut bytes)
            .map_err(|_| Error::SerialisationError)?;

        Ok(bytes)
    }

    /// Attempt to deserialize a `PublicCommitmentShareList` from a vector of bytes.
    pub fn from_bytes(bytes: &[u8]) -> FrostResult<C, Self> {
        Self::deserialize_compressed(bytes).map_err(|_| Error::DeserialisationError)
    }
}

/// Pre-compute a list of [`CommitmentShare`]s for single-round threshold signing.
///
/// # Inputs
///
/// * `participant_index` is the index of the threshold signing
///   participant who is publishing this share.
/// * `number_of_shares` denotes the number of commitments published at a time.
///
/// # Returns
///
/// A tuple of ([`PublicCommitmentShareList`], [`SecretCommitmentShareList`]).
pub fn generate_commitment_share_lists<C: CipherSuite>(
    mut csprng: impl CryptoRng + Rng,
    participant_index: u32,
    number_of_shares: usize,
) -> (PublicCommitmentShareList<C>, SecretCommitmentShareList<C>) {
    let mut commitments: Vec<CommitmentShare<C>> = Vec::with_capacity(number_of_shares);

    for _ in 0..number_of_shares {
        commitments.push(CommitmentShare::from(NoncePair::new(&mut csprng)));
    }

    let mut published: Vec<(C::G, C::G)> = Vec::with_capacity(number_of_shares);

    for commitment in commitments.iter() {
        published.push(commitment.publish());
    }

    (
        PublicCommitmentShareList {
            participant_index,
            commitments: published,
        },
        SecretCommitmentShareList { commitments },
    )
}

impl<C: CipherSuite> SecretCommitmentShareList<C> {
    /// Drop a used [`CommitmentShare`] from our secret commitment share list
    /// and ensure that it is wiped from memory.
    pub fn drop_share(&mut self, share: CommitmentShare<C>) {
        let mut index = -1;

        // This is not constant-time in that the number of commitment shares in
        // the list may be discovered via side channel, as well as the index of
        // share to be deleted, as well as whether or not the share was in the
        // list, but none of this should give any adversary any advantage.
        for (i, s) in self.commitments.iter().enumerate() {
            if s.eq(&share) {
                index = i as isize;
            }
        }
        if index >= 0 {
            drop(self.commitments.remove(index as usize));
        }
        drop(share);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::Secp256k1Sha256;

    use ark_ec::{CurveGroup, Group};
    use ark_ff::UniformRand;
    use ark_secp256k1::{Fr, Projective};
    use rand::rngs::OsRng;

    use core::ops::Mul;

    #[test]
    fn secret_pair() {
        let _secret_pair = NoncePair::<Fr>::new(&mut OsRng);
    }

    #[test]
    fn secret_pair_into_commitment_share() {
        let _commitment_share: CommitmentShare<Secp256k1Sha256> = NoncePair::new(&mut OsRng).into();
    }

    #[test]
    fn test_serialisation() {
        let mut rng = OsRng;

        for _ in 0..100 {
            let secret = Fr::rand(&mut rng);
            let commit = Projective::generator().mul(secret);
            let commitment = Commitment::<Secp256k1Sha256> { secret, commit };
            let mut bytes = Vec::new();

            commitment.serialize_compressed(&mut bytes).unwrap();
            assert_eq!(
                commitment,
                Commitment::deserialize_compressed(&bytes[..]).unwrap()
            );
        }

        for _ in 0..100 {
            let secret = Fr::rand(&mut rng);
            let commit = Projective::generator().mul(secret);
            let binding = Commitment::<Secp256k1Sha256> { secret, commit };
            let hiding = binding.clone();
            let commitment_share = CommitmentShare { binding, hiding };
            let mut bytes = Vec::new();

            commitment_share.serialize_compressed(&mut bytes).unwrap();
            assert_eq!(
                commitment_share,
                CommitmentShare::deserialize_compressed(&bytes[..]).unwrap()
            );
        }

        // invalid encodings
        let bytes = [255u8; 64];
        assert!(Commitment::<Secp256k1Sha256>::deserialize_compressed(&bytes[..]).is_err());

        let bytes = [255u8; 128];
        assert!(CommitmentShare::<Secp256k1Sha256>::deserialize_compressed(&bytes[..]).is_err());
    }

    #[test]
    fn commitment_share_list_generate() {
        let (public_share_list, secret_share_list) =
            generate_commitment_share_lists::<Secp256k1Sha256>(&mut OsRng, 0, 5);

        assert_eq!(
            public_share_list.commitments[0].0.into_affine(),
            (Projective::generator().mul(secret_share_list.commitments[0].hiding.secret))
                .into_affine()
        );
    }

    #[test]
    fn drop_used_commitment_shares() {
        let (_public_share_list, mut secret_share_list) =
            generate_commitment_share_lists(&mut OsRng, 3, 8);

        assert!(secret_share_list.commitments.len() == 8);

        let used_share: CommitmentShare<Secp256k1Sha256> = secret_share_list.commitments[0].clone();

        secret_share_list.drop_share(used_share);

        assert!(secret_share_list.commitments.len() == 7);
    }
}