{-# LANGUAGE DeriveGeneric #-}

module Concordium.Types.SeedState where

import Data.Serialize
import GHC.Generics

import Concordium.Crypto.SHA256 (Hash)
import Concordium.Types

-- |State for computing the leadership election nonce.
data SeedState = SeedState
    { -- |Number of slots in an epoch. This is derived from genesis
      -- data and must not change.
      epochLength :: !EpochLength,
      -- |Current epoch
      epoch :: !Epoch,
      -- |Current leadership election nonce
      currentLeadershipElectionNonce :: !LeadershipElectionNonce,
      -- |The leadership election nonce updated with the block nonces
      -- of blocks in the first 2/3 of the current epoch.
      updatedNonce :: !Hash
    }
    deriving (Eq, Generic, Show)

instance Serialize SeedState

-- |Instantiate a seed state: leadership election nonce should be random, epoch length should be long, but not too long...
initialSeedState :: LeadershipElectionNonce -> EpochLength -> SeedState
initialSeedState nonce epochLength =
    SeedState
        { epoch = 0,
          currentLeadershipElectionNonce = nonce,
          updatedNonce = nonce,
          ..
        }
