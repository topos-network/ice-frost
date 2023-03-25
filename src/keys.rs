use core::marker::PhantomData;
use core::ops::{Deref, Mul};

use crate::dkg::secret_share::VerifiableSecretSharingCommitment;
use crate::utils::calculate_lagrange_coefficients;
use crate::utils::{ToString, Vec};
use crate::{Error, FrostResult};

use crate::ciphersuite::CipherSuite;

use ark_ec::{CurveGroup, Group};
use ark_ff::Zero;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};

use zeroize::Zeroize;

/// A Diffie-Hellman private key wrapper type around a PrimeField.
#[derive(Clone, Debug, Eq, PartialEq, CanonicalSerialize, CanonicalDeserialize, Zeroize)]
pub struct DiffieHellmanPrivateKey<C: CipherSuite>(pub(crate) <C::G as Group>::ScalarField);

impl<C: CipherSuite> DiffieHellmanPrivateKey<C> {
    /// Serialize this `DiffieHellmanPrivateKey` to a vector of bytes.
    pub fn to_bytes(&self) -> FrostResult<C, Vec<u8>> {
        let mut bytes = Vec::new();

        self.serialize_compressed(&mut bytes)
            .map_err(|_| Error::SerialisationError)?;

        Ok(bytes)
    }

    /// Attempt to deserialize a `DiffieHellmanPrivateKey` from a vector of bytes.
    pub fn from_bytes(bytes: &[u8]) -> FrostResult<C, Self> {
        Self::deserialize_compressed(bytes).map_err(|_| Error::DeserialisationError)
    }
}

impl<C: CipherSuite> Drop for DiffieHellmanPrivateKey<C> {
    fn drop(&mut self) {
        self.zeroize();
    }
}

/// A Diffie-Hellman public key wrapper type around a CurveGroup.
#[derive(Clone, Debug, Eq, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct DiffieHellmanPublicKey<C: CipherSuite> {
    pub(crate) key: C::G,
    _phantom: PhantomData<C>,
}

impl<C: CipherSuite> DiffieHellmanPublicKey<C> {
    /// Instantiates a new `DiffieHellmanPublicKey` key.
    pub fn new(key: C::G) -> Self {
        Self {
            key,
            _phantom: PhantomData,
        }
    }

    /// Serialize this `DiffieHellmanPublicKey` to a vector of bytes.
    pub fn to_bytes(&self) -> FrostResult<C, Vec<u8>> {
        let mut bytes = Vec::new();

        self.serialize_compressed(&mut bytes)
            .map_err(|_| Error::SerialisationError)?;

        Ok(bytes)
    }

    /// Attempt to deserialize a `DiffieHellmanPublicKey` from a vector of bytes.
    pub fn from_bytes(bytes: &[u8]) -> FrostResult<C, Self> {
        Self::deserialize_compressed(bytes).map_err(|_| Error::DeserialisationError)
    }
}

impl<C: CipherSuite> Deref for DiffieHellmanPublicKey<C> {
    type Target = C::G;

    fn deref(&self) -> &Self::Target {
        &self.key
    }
}

/// A public verification share for a participant.
///
/// Any participant can recalculate the public verification share, which is the
/// public half of a [`IndividualSigningKey`], of any other participant in the protocol.
#[derive(Clone, Debug, Eq, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct IndividualVerifyingKey<C: CipherSuite> {
    /// The participant index to which this key belongs.
    pub index: u32,
    /// The public verification share.
    pub share: <C as CipherSuite>::G,
}

impl<C: CipherSuite> IndividualVerifyingKey<C> {
    /// Serialize this `IndividualVerifyingKey` to a vector of bytes.
    pub fn to_bytes(&self) -> FrostResult<C, Vec<u8>> {
        let mut bytes = Vec::new();

        self.serialize_compressed(&mut bytes)
            .map_err(|_| Error::SerialisationError)?;

        Ok(bytes)
    }

    /// Attempt to deserialize a `IndividualVerifyingKey` from a vector of bytes.
    pub fn from_bytes(bytes: &[u8]) -> FrostResult<C, Self> {
        Self::deserialize_compressed(bytes).map_err(|_| Error::DeserialisationError)
    }

    /// Any participant can compute the public verification share of any other participant.
    ///
    /// This is done by re-computing each [`IndividualVerifyingKey`] as \\(Y\_i\\) s.t.:
    ///
    /// \\[
    /// Y\_i = \prod\_{j=1}^{n} \prod\_{k=0}^{t-1} \phi\_{jk}^{i^{k} \mod q}
    /// \\]
    ///
    /// for each [`Participant`] index \\(i\\).
    ///
    /// # Inputs
    ///
    /// * A vector of `commitments` regarding the secret polynomial
    ///   [`Coefficients`] that this [`IndividualVerifyingKey`] was generated with.
    ///
    /// # Returns
    ///
    /// A `Result` with either an empty `Ok` or `Err` value, depending on
    /// whether or not the verification was successful.
    pub fn verify(
        &self,
        commitments: &[VerifiableSecretSharingCommitment<C>],
    ) -> FrostResult<C, ()> {
        let mut rhs: C::G = <C as CipherSuite>::G::zero();
        let term: <C::G as Group>::ScalarField = self.index.into();

        let mut index_vector: Vec<u32> = Vec::new();
        for commitment in commitments.iter() {
            index_vector.push(commitment.index);
        }

        for commitment in commitments.iter() {
            let mut tmp: C::G = <C as CipherSuite>::G::zero();
            for (index, com) in commitment.points.iter().rev().enumerate() {
                tmp += com;

                if index != (commitment.points.len() - 1) {
                    tmp *= term;
                }
            }

            let coeff = match calculate_lagrange_coefficients::<C>(commitment.index, &index_vector)
            {
                Ok(s) => s,
                Err(error) => return Err(Error::Custom(error.to_string())),
            };

            rhs += tmp.mul(coeff);
        }

        match self.share.into_affine() == rhs.into_affine() {
            true => Ok(()),
            false => Err(Error::ShareVerificationError),
        }
    }

    /// Any participant can compute the public verification share of any other participant.
    ///
    /// This is done by re-computing each [`IndividualVerifyingKey`] as \\(Y\_i\\) s.t.:
    ///
    /// \\[
    /// Y\_i = \prod\_{j=1}^{n} \prod\_{k=0}^{t-1} \phi\_{jk}^{i^{k} \mod q}
    /// \\]
    ///
    /// for each [`Participant`] index \\(i\\).
    ///
    /// # Inputs
    ///
    /// * A `participant_index` and
    /// * A vector of `commitments` regarding the secret polynomial
    ///   [`Coefficients`] that the [`IndividualVerifyingKey`] will be generated from.
    ///
    /// # Returns
    ///
    /// An `IndividualVerifyingKey`.
    pub fn generate_from_commitments(
        participant_index: u32,
        commitments: &[VerifiableSecretSharingCommitment<C>],
    ) -> Self {
        let mut share: C::G = <C as CipherSuite>::G::zero();
        let term: <C::G as Group>::ScalarField = participant_index.into();

        let mut index_vector: Vec<u32> = Vec::new();
        for commitment in commitments.iter() {
            index_vector.push(commitment.index);
        }

        for commitment in commitments.iter() {
            let mut tmp: C::G = <C as CipherSuite>::G::zero();
            for (index, com) in commitment.points.iter().rev().enumerate() {
                tmp += com;

                if index != (commitment.points.len() - 1) {
                    tmp *= term;
                }
            }

            let coeff =
                calculate_lagrange_coefficients::<C>(commitment.index, &index_vector).unwrap();
            share += tmp * coeff;
        }

        IndividualVerifyingKey {
            index: participant_index,
            share,
        }
    }
}

/// A secret key, used by one participant in a threshold signature scheme, to sign a message.
#[derive(Clone, Debug, Eq, PartialEq, CanonicalSerialize, CanonicalDeserialize, Zeroize)]
pub struct IndividualSigningKey<C: CipherSuite> {
    /// The participant index to which this key belongs.
    pub(crate) index: u32,
    /// The participant's long-lived secret share of the group signing key.
    pub(crate) key: <C::G as Group>::ScalarField,
}

impl<C: CipherSuite> IndividualSigningKey<C> {
    /// Serialize this `IndividualSigningKey` to a vector of bytes.
    pub fn to_bytes(&self) -> FrostResult<C, Vec<u8>> {
        let mut bytes = Vec::new();

        self.serialize_compressed(&mut bytes)
            .map_err(|_| Error::SerialisationError)?;

        Ok(bytes)
    }

    /// Attempt to deserialize a `IndividualSigningKey` from a vector of bytes.
    pub fn from_bytes(bytes: &[u8]) -> FrostResult<C, Self> {
        Self::deserialize_compressed(bytes).map_err(|_| Error::DeserialisationError)
    }
}

impl<C: CipherSuite> Drop for IndividualSigningKey<C> {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl<C: CipherSuite> IndividualSigningKey<C> {
    /// Derive the corresponding public key for this secret key.
    pub fn to_public(&self) -> IndividualVerifyingKey<C> {
        let share = C::G::generator() * self.key;

        IndividualVerifyingKey {
            index: self.index,
            share,
        }
    }
}

impl<C: CipherSuite> From<&IndividualSigningKey<C>> for IndividualVerifyingKey<C> {
    fn from(source: &IndividualSigningKey<C>) -> IndividualVerifyingKey<C> {
        source.to_public()
    }
}

/// A public key, used to verify a signature made by a threshold of a group of participants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, CanonicalSerialize, CanonicalDeserialize)]
pub struct GroupKey<C: CipherSuite> {
    pub(crate) key: C::G,
    _phantom: PhantomData<C>,
}

impl<C: CipherSuite> GroupKey<C> {
    /// Instantiates a new `GroupKey` key.
    pub fn new(key: C::G) -> Self {
        Self {
            key,
            _phantom: PhantomData,
        }
    }

    /// Serialize this `GroupKey` to a vector of bytes.
    pub fn to_bytes(&self) -> FrostResult<C, Vec<u8>> {
        let mut bytes = Vec::new();

        self.serialize_compressed(&mut bytes)
            .map_err(|_| Error::SerialisationError)?;

        Ok(bytes)
    }

    /// Attempt to deserialize a `GroupKey` from a vector of bytes.
    pub fn from_bytes(bytes: &[u8]) -> FrostResult<C, Self> {
        Self::deserialize_compressed(bytes).map_err(|_| Error::DeserialisationError)
    }
}