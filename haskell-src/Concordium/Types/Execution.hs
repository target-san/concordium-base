{-# LANGUAGE EmptyCase #-}
{-# LANGUAGE RecordWildCards #-}
{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE FlexibleInstances #-}
{-# LANGUAGE TypeSynonymInstances #-}
{-# LANGUAGE PatternSynonyms #-}
{-# LANGUAGE DeriveGeneric #-}
module Concordium.Types.Execution where

import Prelude hiding(fail)

import Control.Monad.Reader

import qualified Data.Serialize.Put as P
import qualified Data.Serialize.Get as G
import qualified Data.Serialize as S
import qualified Data.ByteString as BS
import qualified Data.ByteString.Short as BSS
import GHC.Generics

import qualified Concordium.Types.Acorn.Core as Core
import Concordium.Types
import Concordium.ID.Types
import Concordium.Types.Acorn.Interfaces
import qualified Concordium.ID.Types as IDTypes
import Concordium.Crypto.Proofs

-- |These are the messages that are generated as parts of contract execution.
data InternalMessage annot = TSend !ContractAddress !Amount !(Value annot) | TSimpleTransfer !Address !Amount
    deriving(Show)

type Proof = BS.ByteString

-- |We assume that the list is non-empty and at most 255 elements long.
newtype AccountOwnershipProof = AccountOwnershipProof [(KeyIndex, Dlog25519Proof)]
    deriving(Eq, Show)

-- |Helper for when an account has only one key with index 0.
singletonAOP :: Dlog25519Proof -> AccountOwnershipProof
singletonAOP proof = AccountOwnershipProof [(0, proof)]

instance S.Serialize AccountOwnershipProof where
  put (AccountOwnershipProof proofs) = do
    S.putWord8 (fromIntegral (length proofs))
    forM_ proofs (S.putTwoOf S.put S.put)

  get = do
    l <- S.getWord8
    when (l == 0) $ fail "At least one proof must be provided."
    AccountOwnershipProof <$> replicateM (fromIntegral l) (S.getTwoOf S.get S.get)


-- |The transaction payload. Defines the supported kinds of transactions.
--
--  * @SPEC: <$DOCS/Transactions#transaction-body>
--  * @COMMENT: Serialization format is defined separately, this only defines the datatype.
data Payload = 
  -- |Put module on the chain.
  DeployModule {
    -- |Module source.
    dmMod :: !(Core.Module Core.UA)
    }
  -- |Initialize a new contract instance.
  | InitContract {
      -- |Initial amount on the contract's account.
      icAmount :: !Amount,
      -- |Reference of the module (on-chain) in which the contract exist.
      icModRef :: !Core.ModuleRef,
      -- |Name of the contract (relative to the module) to initialize.
      icContractName :: !Core.TyName,
      -- |Parameter to the init method. Relative to the module (as if it were a term at the end of the module).
      icParam :: !(Core.Expr Core.UA Core.ModuleName)
      }
  -- |Update an existing contract instance.
  | Update {
      -- |Amount to call the receive method with.
      uAmount :: !Amount,
      -- |The address of the contract to invoke.
      uAddress :: !ContractAddress,
      -- |Message to invoke the receive method with.
      uMessage :: !(Core.Expr Core.UA Core.ModuleName)
      }
  -- |Simple transfer from an account to either a contract or an account.
  | Transfer {
      -- |Recepient.
      tToAddress :: !Address,
      -- |Amount to transfer.
      tAmount :: !Amount
      }
  -- |Deploy credentials, creating a new account if one does not yet exist.
  | DeployCredential {
      -- |The credentials to deploy.
      dcCredential :: !IDTypes.CredentialDeploymentInformation
      }
  -- |Deploy an encryption key to an existing account.
  | DeployEncryptionKey {
      -- |The encryption key to deploy.
      dekKey :: !IDTypes.AccountEncryptionKey
      }
  -- |Add a new baker with fresh id.
  | AddBaker {
      -- NOTE: The baker id should probably be generated automatically.
      -- we do not wish to recycle baker ids. If we allowed that then
      -- potentially when bakers are removed dishonest bakers might try to
      -- claim their ids and thus abuse the system.
      -- |Public key to verify the baker has won the election.
      abElectionVerifyKey :: !BakerElectionVerifyKey,
      -- |Public key to verify block signatures signed by the baker.
      abSignatureVerifyKey :: !BakerSignVerifyKey,
      -- |Address of the account the baker wants to be rewarded to.
      abAccount :: !AccountAddress,
      -- |Proof that the baker owns the private key corresponding to the
      -- signature verification key.
      abProofSig :: !Dlog25519Proof,
      -- |Proof that the baker owns the private key corresponding to the
      -- election verification key.
      abProofElection :: !Dlog25519Proof,
      -- |Proof that the baker owns the privte key corresponding to the reward
      -- account public key. This is needed at least for beta where we want to
      -- control who can become a baker and thus cannot allow users to send
      -- create their own bakers.
      -- TODO: We could also alternatively just require a signature from one of the
      -- beta accounts on the public data.
      abProofAccount :: !AccountOwnershipProof
      -- FIXME: in the future also logic the baker is allowed to become a baker:
      -- THIS NEEDS SPEC
      }
  -- |Remove an existing baker from the baker pool.
  | RemoveBaker {
      -- |Id of the baker to remove.
      rbId :: !BakerId,
      -- |Proof that we are allowed to remove the baker. One
      -- mechanism would be that the baker would remove itself only
      -- (the transaction must come from the baker's account) but
      -- possibly we want other mechanisms.
      rbProof :: !Proof
      }
  -- |Update the account the baker receives their baking reward to.
  | UpdateBakerAccount {
      -- |Id of the baker to update.
      ubaId :: !BakerId,
      -- |Address of the new account. The account must exist.
      ubaAddress :: !AccountAddress,
      -- |Proof that the baker owns the new account.
      ubaProof :: !AccountOwnershipProof
      }
  -- |Update the signature (verification) key of the baker.
  | UpdateBakerSignKey {
      -- |Id of the baker to update.
      ubsId :: !BakerId,
      -- |New signature verification key.
      ubsKey :: !BakerSignVerifyKey,
      -- |Proof that the baker knows the private key of this verification key.
      ubsProof :: !Dlog25519Proof
      }
  -- |Change which baker an account's stake is delegated to.
  -- If the ID is not valid, the delegation is not updated.
  | DelegateStake {
      -- |ID of the baker to delegate stake to.
      dsID :: !BakerId
      }
  -- |Undelegate stake.
  | UndelegateStake
  deriving(Eq, Show)

-- |Payload serialization according to
--
--  * @SPEC: <$DOCS/Transactions#transaction-body>
instance S.Serialize Payload where
  put DeployModule{..} =
    P.putWord8 0 <>
    Core.putModule dmMod
  put InitContract{..} =
      P.putWord8 1 <>
      S.put icAmount <>
      putModuleRef icModRef <>
      Core.putTyName icContractName <>
      Core.putExpr icParam
  put Update{..} =
    P.putWord8 2 <>
    S.put uAmount <>
    S.put uAddress <>
    Core.putExpr uMessage
  put Transfer{..} =
    P.putWord8 3 <>
    S.put tToAddress <>
    S.put tAmount
  put DeployCredential{..} =
    P.putWord8 4 <>
    S.put dcCredential
  put DeployEncryptionKey{..} =
    P.putWord8 5 <>
    S.put dekKey
  put AddBaker{..} =
    P.putWord8 6 <>
    S.put abElectionVerifyKey <>
    S.put abSignatureVerifyKey <>
    S.put abAccount <>
    S.put abProofSig <>
    S.put abProofElection <>
    S.put abProofAccount
  put RemoveBaker{..} =
    P.putWord8 7 <>
    S.put rbId <>
    S.put rbProof
  put UpdateBakerAccount{..} =
    P.putWord8 8 <>
    S.put ubaId <>
    S.put ubaAddress <>
    S.put ubaProof
  put UpdateBakerSignKey{..} =
    P.putWord8 9 <>
    S.put ubsId <>
    S.put ubsKey <>
    S.put ubsProof
  put DelegateStake{..} =
    P.putWord8 10 <>
    S.put dsID
  put UndelegateStake =
    P.putWord8 11

  get = do
    G.getWord8 >>=
      \case 0 -> do
              dmMod <- Core.getModule
              return DeployModule{..}
            1 -> do
              icAmount <- S.get
              icModRef <- getModuleRef
              icContractName <- Core.getTyName
              icParam <- Core.getExpr
              return InitContract{..}
            2 -> do
              uAmount <- S.get
              uAddress <- S.get
              uMessage <- Core.getExpr
              return Update{..}
            3 -> do
              tToAddress <- S.get
              tAmount <- S.get
              return Transfer{..}
            4 -> do
              dcCredential <- S.get
              return DeployCredential{..}
            5 -> do
              dekKey <- S.get
              return DeployEncryptionKey{..}
            6 -> do
              abElectionVerifyKey <- S.get
              abSignatureVerifyKey <- S.get
              abAccount <- S.get
              abProofSig <- S.get
              abProofElection <- S.get
              abProofAccount <- S.get
              return AddBaker{..}
            7 -> do
              rbId <- S.get
              rbProof <- S.get
              return RemoveBaker{..}
            8 -> do
              ubaId <- S.get
              ubaAddress <- S.get
              ubaProof <- S.get
              return UpdateBakerAccount{..}
            9 -> do
              ubsId <- S.get
              ubsKey <- S.get
              ubsProof <- S.get
              return UpdateBakerSignKey{..}
            10 -> DelegateStake <$> S.get
            11 -> return UndelegateStake
            _ -> fail "Unsupported transaction type."

{-# INLINE encodePayload #-}
encodePayload :: Payload -> EncodedPayload
encodePayload = EncodedPayload . BSS.toShort . S.encode

{-# INLINE decodePayload #-}
decodePayload :: EncodedPayload -> Either String Payload
decodePayload (EncodedPayload s) = S.decode (BSS.fromShort s)

{-# INLINE payloadBodyBytes #-}
-- |Get the body of the payload as bytes. Essentially just remove the
-- first byte which encodes the type.
payloadBodyBytes :: EncodedPayload -> BS.ByteString
payloadBodyBytes (EncodedPayload ss) =
  if BSS.null ss
  then BS.empty
  else BS.tail (BSS.fromShort ss)

-- |Events which are generated during transaction execution.
-- These are only used for commited transactions.
data Event = ModuleDeployed !Core.ModuleRef
           | ContractInitialized !Core.ModuleRef !Core.TyName !ContractAddress
           | Updated !Address !ContractAddress !Amount !MessageFormat
           | Transferred !Address !Amount !Address
           | AccountCreated !AccountAddress
           | CredentialDeployed !IDTypes.CredentialDeploymentValues
           | AccountEncryptionKeyDeployed AccountAddress IDTypes.AccountEncryptionKey
           | BakerAdded !BakerId
           | BakerRemoved !BakerId
           | BakerAccountUpdated !BakerId !AccountAddress
           | BakerKeyUpdated !BakerId !BakerSignVerifyKey
           | StakeDelegated !AccountAddress !BakerId
           | StakeUndelegated !AccountAddress
  deriving (Show, Generic, Eq)

instance S.Serialize Event

-- |Used internally by the scheduler since internal messages are sent as values,
-- and top-level messages are acorn expressions.
data MessageFormat = ValueMessage !(Value Core.NoAnnot) | ExprMessage !(LinkedExpr Core.NoAnnot)
    deriving(Show, Generic, Eq)

instance S.Serialize MessageFormat where
    put (ValueMessage v) = S.putWord8 0 >> putStorable v
    put (ExprMessage e) = S.putWord8 1 >> S.put e
    get = do
        tag <- S.getWord8
        case tag of
            0 -> ValueMessage <$> getStorable
            1 -> ExprMessage <$> S.get
            _ -> fail "Invalid MessageFormat tag"

-- |Result of a valid transaction is either a reject with a reason or a
-- successful transaction with a list of events which occurred during execution.
-- We also record the cost of the transaction.
data ValidResult =
  TxSuccess {
    vrEvents :: ![Event],
    vrTransactionCost :: !Amount,
    vrEnergyCost :: !Energy
  } |
  TxReject {
    vrRejectReason :: !RejectReason,
    vrTransactionCost :: !Amount,
    vrEnergyCost :: !Energy
  }
  deriving(Show, Generic, Eq)

instance S.Serialize ValidResult


-- |Ways a single transaction can fail. Values of this type are only used for reporting of rejected transactions.
data RejectReason = ModuleNotWF -- ^Error raised when typechecking of the module has failed.
                  | MissingImports  -- ^Error when there were missing imports (determined before typechecking).
                  | ModuleHashAlreadyExists !Core.ModuleRef  -- ^As the name says.
                  | MessageTypeError -- ^Message to the receive method is of the wrong type.
                  | ParamsTypeError -- ^Parameters of the init method are of the wrong type.
                  | InvalidAccountReference !AccountAddress -- ^Account does not exists.
                  | InvalidContractReference !Core.ModuleRef !Core.TyName -- ^Reference to a non-existing contract.
                  | InvalidModuleReference !Core.ModuleRef   -- ^Reference to a non-existing module.
                  | InvalidContractAddress !ContractAddress -- ^Contract instance does not exist.
                  | ReceiverAccountNoCredential !AccountAddress
                  -- ^The receiver account does not have a valid credential.
                  | ReceiverContractNoCredential !ContractAddress
                  -- ^The receiver contract does not have a valid credential.
                  | EvaluationError         -- ^Error during evalution. This is
                                            -- mostly for debugging purposes
                                            -- since this kind of an error should
                                            -- not happen after successful
                                            -- typechecking.
                  | AmountTooLarge !Address !Amount
                  -- ^When one wishes to transfer an amount from A to B but there
                  -- are not enough funds on account/contract A to make this
                  -- possible. The data are the from address and the amount to transfer.
                  | SerializationFailure String -- ^Serialization of the body failed for the given reason.
                  | OutOfEnergy -- ^We ran of out energy to process this transaction.
                  | Rejected -- ^Rejected due to contract logic.
                  | DuplicateAccountRegistrationID IDTypes.CredentialRegistrationID
                  | NonExistentIdentityProvider !IDTypes.IdentityProviderIdentity
                  | AccountCredentialInvalid
                  | AccountEncryptionKeyAlreadyExists AccountAddress IDTypes.AccountEncryptionKey
                  | NonExistentRewardAccount !AccountAddress -- ^Reward account desired by the baker does not exist.
                  | InvalidProof -- ^Proof that the baker owns relevant private keys is not valid.
                  | RemovingNonExistentBaker !BakerId
                  | InvalidBakerRemoveSource !AccountAddress
                  | UpdatingNonExistentBaker !BakerId
                  | InvalidStakeDelegationTarget !BakerId -- ^The target of stake delegation is not a valid baker.
                  | DuplicateSignKey !BakerSignVerifyKey -- ^A baker with the given signing key already exists.
                  -- |A transaction should be sent from the baker's current account, but is not.
                  | NotFromBakerAccount { nfbaFromAccount :: !AccountAddress, -- ^Sender account of the transaction
                                          nfbaCurrentBakerAccount :: !AccountAddress -- ^Current baker account.
                                        }
    deriving (Show, Eq, Generic)

instance S.Serialize RejectReason

data FailureKind = InsufficientFunds   -- ^The amount is not sufficient to cover the gas deposit.
                 | IncorrectSignature  -- ^Signature check failed.
                 | NonSequentialNonce !Nonce -- ^The transaction nonce is not
                                             -- next in sequence. The argument
                                             -- is the expected nonce.
                 | UnknownAccount !AccountAddress -- ^Transaction is coming from an unknown sender.
                 | DepositInsufficient -- ^The dedicated gas amount was lower than the minimum allowed.
                 | NoValidCredential -- ^No valid credential on the sender account.
      deriving(Eq, Show)

data TxResult = TxValid ValidResult | TxInvalid FailureKind
