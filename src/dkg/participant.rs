use ark_ec::Group;
use ark_ff::UniformRand;
use ark_serialize::CanonicalDeserialize;
use ark_serialize::CanonicalSerialize;

use core::cmp::Ordering;
use core::ops::Mul;
use rand::CryptoRng;
use rand::RngCore;

use crate::ciphersuite::CipherSuite;
use crate::dkg::{
    secret_share::{Coefficients, EncryptedSecretShare, VerifiableSecretSharingCommitment},
    NizkPokOfSecretKey,
};
use crate::keys::{DiffieHellmanPrivateKey, DiffieHellmanPublicKey, IndividualSigningKey};
use crate::parameters::ThresholdParameters;
use crate::{Error, FrostResult};

use crate::utils::Vec;

use super::DKGParticipantList;
use super::DistributedKeyGeneration;

/// A participant in a threshold signing.
#[derive(Clone, Debug, CanonicalSerialize, CanonicalDeserialize)]
pub struct Participant<C: CipherSuite> {
    /// The index of this participant, to keep the participants in order.
    pub index: u32,
    /// The public key used to derive symmetric keys for encrypting and
    /// decrypting shares via DH.
    pub dh_public_key: DiffieHellmanPublicKey<C>,
    /// A vector of Pedersen commitments to the coefficients of this
    /// participant's private polynomial.
    pub commitments: Option<VerifiableSecretSharingCommitment<C>>,
    /// The zero-knowledge proof of knowledge of the secret key (a.k.a. the
    /// first coefficient in the private polynomial).  It is constructed as a
    /// Schnorr signature using \\( a_{i0} \\) as the signing key.
    pub proof_of_secret_key: Option<NizkPokOfSecretKey<C>>,
    /// The zero-knowledge proof of knowledge of the DH private key.
    /// It is computed similarly to the proof_of_secret_key.
    pub proof_of_dh_private_key: NizkPokOfSecretKey<C>,
}

impl<C: CipherSuite> Participant<C>
where
    [(); C::HASH_SEC_PARAM]:,
{
    /// Construct a new dealer for the distributed key generation protocol,
    /// who will generate shares for a group of signers (can be the group of dealers).
    ///
    /// In case of resharing/refreshing of the secret participant shares once the
    /// Dkg has completed, a dealer can call the [`reshare`] method to distribute
    /// shares of her secret key to a new set of participants.
    ///
    /// # Inputs
    ///
    /// * The protocol instance [`ThresholdParameters`],
    /// * This participant's [`index`],
    /// * A context string to prevent replay attacks.
    ///
    /// # Usage
    ///
    /// After a new participant is constructed, the [`participant.index`],
    /// [`participant.commitments`], [`participant.proof_of_secret_key`] and
    /// [`participant.proof_of_dh_private_key`] should be sent to every
    /// other participant in the protocol.
    ///
    /// # Returns
    ///
    /// A distributed key generation protocol [`Participant`] and that
    /// dealer's secret polynomial [`Coefficients`] along the dealer's
    /// Diffie-Hellman private key for secret shares encryption which
    /// must be kept private.
    pub fn new_dealer(
        parameters: &ThresholdParameters<C>,
        index: u32,
        mut rng: impl RngCore + CryptoRng,
    ) -> (Self, Coefficients<C>, DiffieHellmanPrivateKey<C>) {
        let (dealer, coeff_option, dh_private_key) =
            Self::new_internal(parameters, false, index, None, &mut rng);
        (dealer, coeff_option.unwrap(), dh_private_key)
    }

    /// Construct a new signer for the distributed key generation protocol.
    ///
    /// A signer only combines shares from a previous set of dealers and
    /// computes a private signing key from it.
    ///
    /// # Inputs
    ///
    /// * The protocol instance [`ThresholdParameters`],
    /// * This participant's [`index`],
    /// * A context string to prevent replay attacks.
    ///
    /// # Usage
    ///
    /// After a new participant is constructed, the [`participant.index`
    /// and [`participant.proof_of_dh_private_key`] should be sent to every
    /// other participant in the protocol.
    ///
    /// # Returns
    ///
    /// A distributed key generation protocol [`Participant`] along the
    /// signers's Diffie-Hellman private key for secret shares encryption
    /// which must be kept private,
    pub fn new_signer(
        parameters: &ThresholdParameters<C>,
        index: u32,
        mut rng: impl RngCore + CryptoRng,
    ) -> (Self, DiffieHellmanPrivateKey<C>) {
        let (signer, _coeff_option, dh_private_key) =
            Self::new_internal(parameters, true, index, None, &mut rng);
        (signer, dh_private_key)
    }

    fn new_internal(
        parameters: &ThresholdParameters<C>,
        is_signer: bool,
        index: u32,
        secret_key: Option<<C::G as Group>::ScalarField>,
        mut rng: impl RngCore + CryptoRng,
    ) -> (Self, Option<Coefficients<C>>, DiffieHellmanPrivateKey<C>) {
        // Step 1: Every participant P_i samples t random values (a_{i0}, ..., a_{i(t-1)})
        //         uniformly in ZZ_q, and uses these values as coefficients to define a
        //         polynomial f_i(x) = \sum_{j=0}^{t-1} a_{ij} x^{j} of degree t-1 over
        //         ZZ_q.
        let t: usize = parameters.t as usize;

        // RICE-FROST: Every participant samples a random pair of keys (dh_private_key, dh_public_key)
        // and generates a proof of knowledge of dh_private_key. This will be used for secret shares
        // encryption and for complaint generation.

        let dh_private_key = DiffieHellmanPrivateKey(<C::G as Group>::ScalarField::rand(&mut rng));
        let dh_public_key = DiffieHellmanPublicKey::new(C::G::generator().mul(dh_private_key.0));

        // Compute a proof of knowledge of dh_secret_key
        // TODO: error
        let proof_of_dh_private_key =
            NizkPokOfSecretKey::<C>::prove(index, &dh_private_key.0, &dh_public_key, &mut rng)
                .unwrap();

        if is_signer {
            // Signers don't need coefficients, commitments or proofs of secret key.
            (
                Participant {
                    index,
                    dh_public_key,
                    commitments: None,
                    proof_of_secret_key: None,
                    proof_of_dh_private_key,
                },
                None,
                dh_private_key,
            )
        } else {
            let mut coefficients: Vec<<C::G as Group>::ScalarField> = Vec::with_capacity(t);
            let mut commitments = VerifiableSecretSharingCommitment {
                index,
                points: Vec::with_capacity(t),
            };

            match secret_key {
                Some(sk) => coefficients.push(sk),
                None => coefficients.push(<C::G as Group>::ScalarField::rand(&mut rng)),
            }

            for _ in 1..t {
                coefficients.push(<C::G as Group>::ScalarField::rand(&mut rng));
            }

            let coefficients = Coefficients(coefficients);

            // Step 3: Every dealer computes a public commitment
            //         C_i = [\phi_{i0}, ..., \phi_{i(t-1)}], where \phi_{ij} = g^{a_{ij}},
            //         0 ≤ j ≤ t-1.
            for j in 0..t {
                commitments
                    .points
                    .push(C::G::generator() * coefficients.0[j]);
            }

            // The steps are out of order, in order to save one scalar multiplication.

            // Step 2: Every dealer computes a proof of knowledge to the corresponding secret
            //         a_{i0} by calculating a Schnorr signature \alpha_i = (s, group_commitment).  (In
            //         the FROST paper: \alpha_i = (\mu_i, c_i), but we stick with Schnorr's
            //         original notation here.)
            // TODO: error
            let proof_of_secret_key: NizkPokOfSecretKey<C> = NizkPokOfSecretKey::prove(
                index,
                &coefficients.0[0],
                commitments.public_key().unwrap(),
                rng,
            )
            .unwrap();

            (
                Participant {
                    index,
                    dh_public_key,
                    commitments: Some(commitments),
                    proof_of_secret_key: Some(proof_of_secret_key),
                    proof_of_dh_private_key,
                },
                Some(coefficients),
                dh_private_key,
            )
        }
    }

    /// Reshare this dealer's secret key to a new set of participants.
    ///
    /// # Inputs
    ///
    /// * The *new* protocol instance [`ThresholdParameters`],
    /// * This participant's [`secret_key`],
    /// * A reference to the list of new participants,
    /// * A context string to prevent replay attacks.
    ///
    /// # Usage
    ///
    /// After a new participant is constructed, the [`participant.index`],
    /// [`participant.commitments`], [`participant.proof_of_secret_key`] and
    /// [`participant.proof_of_dh_private_key`] should be sent to every other
    /// participant in the protocol along with their dedicated secret share.
    ///
    /// # Returns
    ///
    /// A distributed key generation protocol [`Participant`], a
    /// [`Vec<EncryptedSecretShare::<C>>`] to be sent to each participant
    /// of the new set accordingly.
    /// It also returns a list of the valid / misbehaving participants
    /// of the new set for handling outside of this crate.
    pub fn reshare(
        parameters: &ThresholdParameters<C>,
        secret_key: IndividualSigningKey<C>,
        signers: &[Participant<C>],
        mut rng: impl RngCore + CryptoRng,
    ) -> FrostResult<C, (Self, Vec<EncryptedSecretShare<C>>, DKGParticipantList<C>)> {
        let (dealer, coeff_option, dh_private_key) = Self::new_internal(
            parameters,
            false,
            secret_key.index,
            Some(secret_key.key),
            &mut rng,
        );

        // Unwrapping cannot panic here
        let coefficients = coeff_option.unwrap();

        let (participant_state, participant_lists) = DistributedKeyGeneration::new_state_internal(
            parameters,
            &dh_private_key,
            &secret_key.index,
            Some(&coefficients),
            signers,
            true,
            false,
            &mut rng,
        )?;

        // Unwrapping cannot panic here
        let encrypted_shares = participant_state
            .their_encrypted_secret_shares()
            .unwrap()
            .clone();

        Ok((dealer, encrypted_shares, participant_lists))
    }

    /// Serialize this [`Participant`] to a vector of bytes.
    pub fn to_bytes(&self) -> FrostResult<C, Vec<u8>> {
        let mut bytes = Vec::new();

        self.serialize_compressed(&mut bytes)
            .map_err(|_| Error::SerializationError)?;

        Ok(bytes)
    }

    /// Attempt to deserialize a [`Participant`] from a vector of bytes.
    pub fn from_bytes(bytes: &[u8]) -> FrostResult<C, Self> {
        Self::deserialize_compressed(bytes).map_err(|_| Error::DeserializationError)
    }

    /// Retrieve \\( \alpha_{i0} * B \\), where \\( B \\) is the Ristretto basepoint.
    ///
    /// This is used to pass into the final call to [`DistributedKeyGeneration::<RoundTwo>.finish()`] .
    pub fn public_key(&self) -> Option<&C::G> {
        if self.commitments.is_some() {
            return self.commitments.as_ref().unwrap().public_key();
        }

        None
    }
}

impl<C: CipherSuite> PartialOrd for Participant<C> {
    fn partial_cmp(&self, other: &Participant<C>) -> Option<Ordering> {
        match self.index.cmp(&other.index) {
            Ordering::Less => Some(Ordering::Less),
            Ordering::Equal => None, // Participants cannot have the same index.
            Ordering::Greater => Some(Ordering::Greater),
        }
    }
}

impl<C: CipherSuite> PartialEq for Participant<C> {
    fn eq(&self, other: &Participant<C>) -> bool {
        self.index == other.index
    }
}