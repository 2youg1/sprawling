-- This Source Code Form is subject to the terms of the Mozilla Public
-- License, v. 2.0. If a copy of the MPL was not distributed with this
-- file, You can obtain one at https://mozilla.org/MPL/2.0/.
-- Copyright (c) 2026 2youg1 and the sprawling contributors

{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The algebraic mirror of what a socket carries, and nothing else.
--
-- This module parses and does not judge. It knows the five frame classes
-- `channels::ServerFrame` spells and the shape of a refusal; it does not know
-- which refusal is owed, which is "Sprawling.Model"'s business.
--
-- An answer keeps its name and its body as it arrived. Fourteen queries exist
-- and the model drives four, so decoding all fourteen into records would be
-- fourteen restatements of a shape that already has an authority in Rust. The
-- name is checked because a renamed answer is a wire change; the body is read
-- only where something asserts on it.
module Sprawling.Frame
  ( Frame (..)
  , Welcome (..)
  , Complaint (..)
  , Record (..)
  , Code (..)
  , decodeFrame
  , cityBuildings
  , refusalOf
  , recordOf
  ) where

import Data.Aeson (Value (..), (.:), (.:?))
import Data.Aeson qualified as Aeson
import Data.Aeson.Key qualified as Key
import Data.Aeson.KeyMap qualified as KeyMap
import Data.Aeson.Types qualified as Aeson
import Data.ByteString.Lazy qualified as Lazy
import Data.Foldable (toList)
import Data.List (sort)
import Data.Text (Text)
import Data.Word (Word64)

-- | A stable error code, as the door prints it: @E_GATE_DENIED@ and its 34
-- siblings.
--
-- Kept as text rather than as a closed enum on purpose. Mirroring
-- @AxCode::ALL@ here would put a second copy of that table in this
-- repository, and the property this adversary asserts is that a particular
-- refusal carries a particular code — not that the set of codes is what
-- some Haskell file says it is.
newtype Code = Code Text
  deriving stock (Eq, Ord, Show)

-- | What the server said when it took the connection.
data Welcome = Welcome
  { welcomeWire :: Word64
  , welcomeSchema :: Text
  , welcomeCity :: Maybe Text
  }
  deriving stock (Eq, Show)

-- | A refusal in the three parts `kernel::AxError` promises, plus the two
-- fields that travel with them.
data Complaint = Complaint
  { complaintCode :: Code
  , complaintAction :: Text
  , complaintSubject :: Text
  , complaintRecovery :: Text
  , complaintRetriable :: Bool
  }
  deriving stock (Eq, Show)

-- | One line of history, as it was pushed.
data Record = Record
  { recordSeq :: Word64
  , recordPrev :: Text
  , recordKind :: Text
  , recordWho :: Text
  , recordRun :: Text
  , recordData :: Value
  }
  deriving stock (Eq, Show)

-- | Everything a server may send.
data Frame
  = Welcomed Welcome
  | Happened Record
  | -- | The answer's name, and its body unread.
    Answered Text Value
  | Refused Complaint
  | -- | The run it belongs to, and the text so far.
    Streamed Text Text
  deriving stock (Eq, Show)

-- | One line of the door's stdout.
--
-- A line this cannot read is an error rather than an ignored frame: the wire
-- has changed shape, and carrying on would test a program this adversary can no
-- longer read while reporting green.
decodeFrame :: Lazy.ByteString -> Either String Frame
decodeFrame raw = do
  value <- Aeson.eitherDecode raw
  Aeson.parseEither parseFrame value

parseFrame :: Value -> Aeson.Parser Frame
parseFrame = Aeson.withObject "ServerFrame" $ \envelope ->
  case KeyMap.toList envelope of
    [(tag, body)] -> case Key.toText tag of
      "welcome" -> Welcomed <$> parseWelcome body
      "event" -> Happened <$> parseRecord body
      "answer" -> parseAnswer body
      "refusal" -> Refused <$> parseComplaint body
      "delta" -> parseDelta body
      other -> fail ("a frame class this adversary cannot read: " <> show other)
    fields -> fail ("a frame is one tagged object, not " <> show (length fields))

parseWelcome :: Value -> Aeson.Parser Welcome
parseWelcome = Aeson.withObject "Welcome" $ \o ->
  Welcome <$> o .: "wire_v" <*> o .: "schema" <*> o .:? "city"

parseComplaint :: Value -> Aeson.Parser Complaint
parseComplaint = Aeson.withObject "AxError" $ \o ->
  Complaint
    <$> (Code <$> o .: "code")
    <*> o .: "action"
    <*> o .: "subject"
    <*> o .: "recovery"
    <*> o .: "retriable"

parseRecord :: Value -> Aeson.Parser Record
parseRecord = Aeson.withObject "EventRecord" $ \o ->
  Record
    <$> o .: "seq"
    <*> o .: "prev"
    <*> o .: "kind"
    <*> o .: "who"
    <*> o .: "run"
    <*> o .: "data"

-- | An answer is one tagged object whose tag is the query's own name.
parseAnswer :: Value -> Aeson.Parser Frame
parseAnswer = Aeson.withObject "Answer" $ \o ->
  case KeyMap.toList o of
    [(tag, body)] -> pure (Answered (Key.toText tag) body)
    fields -> fail ("an answer is one tagged object, not " <> show (length fields))

parseDelta :: Value -> Aeson.Parser Frame
parseDelta = Aeson.withObject "Delta" $ \o -> Streamed <$> o .: "run" <*> o .: "text"

-- | The addresses a @city_view@ answer lists, sorted.
--
-- Sorted so that a property about which buildings stand does not accidentally
-- depend on the order the city happened to fold them in. Returns 'Nothing'
-- when the body is not a city answer at all, which the caller reports as a
-- door that changed shape rather than as a city with no buildings.
cityBuildings :: Value -> Maybe [Text]
cityBuildings body = do
  object <- asObject body
  listed <- Aeson.parseMaybe (.: "buildings") object
  addresses <- traverse address (toList (listed :: Aeson.Array))
  pure (sort addresses)
  where
    asObject = \case
      Object o -> Just o
      _ -> Nothing
    address entry = asObject entry >>= Aeson.parseMaybe (.: "addr")

-- | The refusal in a batch of frames, if one is there.
refusalOf :: [Frame] -> Maybe Complaint
refusalOf frames = case [complaint | Refused complaint <- frames] of
  (complaint : _) -> Just complaint
  [] -> Nothing

-- | The first record in a batch of frames whose kind is the one named.
recordOf :: Text -> [Frame] -> Maybe Record
recordOf kind frames =
  case [record | Happened record <- frames, recordKind record == kind] of
    (record : _) -> Just record
    [] -> Nothing
