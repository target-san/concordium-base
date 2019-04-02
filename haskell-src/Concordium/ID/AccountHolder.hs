{-# LANGUAGE DeriveGeneric #-}
module Concordium.ID.AccountHolder where

import GHC.Generics
import Concordium.ID.Types
import Data.ByteString.Char8
import Data.ByteString.Random.MWC
import System.IO.Unsafe
import qualified  Concordium.Crypto.Signature as S
import Concordium.Crypto.SignatureScheme
import Concordium.ID.Attributes
import qualified Concordium.Crypto.SHA224 as SHA224
import qualified Data.ByteString as BS
import qualified Data.FixedByteString as FBS
import Data.Base58String.Bitcoin

import Foreign.Storable(peek)
import Foreign.Ptr(castPtr)
import Data.Serialize
import Data.Hashable



-- This is a dummy module for testing
--

createAccount :: S.VerifyKey -> AccountCreationInformation  
createAccount ahc = ACI { aci_regId = regId, 
                          aci_arData = ardata, 
                          aci_ipId = ip, 
                          aci_sigScheme = scheme,
                          aci_verifKey = let (S.VerifyKey x) = ahc in (AccVerifyKey (Ed25519_PK x)),
                          aci_encKey = encKey, 
                          aci_policy = policy,
                          aci_auxData = aux,
                          aci_proof = proof
                        }

accountAddress :: AccountCreationInformation-> AccountAddress
accountAddress aci =  accountAddress' vk sc
    where vk = aci_verifKey aci
          sc = aci_sigScheme aci


addressToBase58 :: AccountAddress -> Base58String
addressToBase58 (AccountAddress x) = fromBytes bs
    where
        bs = FBS.toByteString x

base58decodeAddr :: Base58String -> AccountAddress
base58decodeAddr bs = AccountAddress (FBS.fromByteString (toBytes bs))


accountAddressSize = 21

data AccountAddressSize 
instance FBS.FixedLength AccountAddressSize where
    fixedLength _ = accountAddressSize

newtype AccountAddress =  AccountAddress (FBS.FixedByteString AccountAddressSize)
    deriving(Eq, Ord, Generic)

instance Serialize AccountAddress where
    put (AccountAddress h) = putByteString $ FBS.toByteString h
    get = AccountAddress . FBS.fromByteString <$> getByteString accountAddressSize

instance Hashable AccountAddress where
    hashWithSalt s (AccountAddress b) = hashWithSalt s (FBS.toByteString b)
    hash (AccountAddress b) = unsafeDupablePerformIO $ FBS.withPtr b $ \p -> peek (castPtr p)

instance Show AccountAddress where
  show = show . addressToBase58

accountAddress' :: AccountVerificationKey -> SchemeId -> AccountAddress 
accountAddress' (AccVerifyKey x) (SchemeId y) =  AccountAddress (FBS.fromByteString $ BS.cons y (BS.take (accountAddressSize - 1) bs))
    where 
        (SHA224.Hash r) = SHA224.hash (toByteString x) 
        bs = FBS.toByteString r
      

verifyAccount :: AccountCreationInformation -> Bool 
verifyAccount _ = True


ar :: AnonimityRevoker
ar = AR (AR_ID $ pack "superman") (AR_PK $ unsafePerformIO $ random (fromIntegral 32))

ip = IP_ID $ pack "Mateusz"

ardata = [(AR_ID $ pack "superman", Share $ unsafePerformIO $ random (fromIntegral 32))]

regId = RegIdAcc $ unsafePerformIO $ random (fromIntegral 32)

scheme = SchemeId (fromIntegral 2)


encKey = EncKeyAcc ( unsafePerformIO $ random (fromIntegral 32))

policy = Conj (AtomicBD AgeOver18) (AtomicCitizenship EU) 

aux = pack "aux"

proof = Proof $ pack "proof of bot"


randomAcc = unsafePerformIO $ do keypair <- S.newKeyPair
                                 return $ createAccount (S.verifyKey keypair)

