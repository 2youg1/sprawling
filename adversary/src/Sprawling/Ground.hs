-- This Source Code Form is subject to the terms of the Mozilla Public
-- License, v. 2.0. If a copy of the MPL was not distributed with this
-- file, You can obtain one at https://mozilla.org/MPL/2.0/.
-- Copyright (c) 2026 2youg1 and the sprawling contributors

{-# LANGUAGE DerivingStrategies #-}
{-# LANGUAGE OverloadedStrings #-}

-- | A city that exists for one test, and the disk's power to lie in it.
--
-- A city is one directory plus one process serving it, so a throwaway city is
-- both — this module owns the pair and, through `bracket`, owns their death on
-- every path out.
--
-- Reaching into the ledger directory is not going behind the product's back.
-- ARCHITECTURE section 6 puts the whole history in a tree on this disk, so
-- anybody who can read that directory can write it; taking that position is
-- taking the one the design already grants an adversary. Everything an agent
-- does still goes through "Sprawling.Door".
module Sprawling.Ground
  ( Ground
  , cast
  , withGround
  , portOf
  , cityOf
  , ledgerOf
  , stored
  , tree
  , corrupt
  , tear
  , duplicate
  ) where

import Control.Concurrent (threadDelay)
import Control.Concurrent.MVar (MVar, modifyMVar, modifyMVar_, newMVar)
import Control.Exception (bracket, finally, onException)
import Control.Monad (forM, unless)
import Data.ByteString qualified as ByteString
import Data.List (isInfixOf, isSuffixOf, sort)
import Data.Text (Text)
import System.Directory (doesDirectoryExist, listDirectory)
import System.FilePath ((</>))
import System.IO.Temp (withSystemTempDirectory)
import System.IO.Unsafe (unsafePerformIO)
import System.Process (ProcessHandle, getProcessExitCode, terminateProcess, waitForProcess)

import Sprawling.Door (Answer (..), Door, Port (..))
import Sprawling.Door qualified as Door

-- | The cast. Three addresses are enough for every question this wire can
-- raise: one to work in, one to collide with, and one nobody ever raises.
cast :: [Text]
cast = ["acme", "beta", "gamma"]

-- | One throwaway city: a directory, and the process serving it.
data Ground = Ground
  { groundCity :: FilePath
  , groundPort :: Port
  }
  deriving stock (Eq, Show)

-- | Where the city's files are.
cityOf :: Ground -> FilePath
cityOf = groundCity

-- | Where it is listening.
portOf :: Ground -> Port
portOf = groundPort

-- | The one history, as a directory on this disk.
ledgerOf :: Ground -> FilePath
ledgerOf ground = groundCity ground </> ".sprawling" </> "ledger"

-- | Every port this process may serve a city on, lent out one at a time.
--
-- Sixteen, because the suite runs its groups in parallel and each running group
-- holds a city of its own.
--
-- One authority for who holds a port, rather than a list each ground walks on
-- its own, because a listening port is exclusive and two grounds racing for one
-- do not merely collide — they silently swap cities. The loser's city fails to
-- bind and exits, and the loser's own readiness probe then reaches the winner's
-- city on that same port, so a whole trace runs against another test's history
-- and reports the answer as its own. That is worse than a red run: it was
-- observed turning a known defect green.
ports :: MVar [Port]
ports = unsafePerformIO (newMVar [Port n | n <- [47100 .. 47115]])
{-# NOINLINE ports #-}

-- | Takes a port nobody else in this process holds, waiting if they all are.
claim :: IO Port
claim = do
  taken <- modifyMVar ports free
  case taken of
    Just port -> pure port
    Nothing -> threadDelay 50_000 >> claim
  where
    free (port : rest) = pure (rest, Just port)
    free [] = pure ([], Nothing)

-- | Hands a port back, after the city that held it is dead and reaped.
release :: Port -> IO ()
release port = modifyMVar_ ports (pure . (port :))

-- | Runs an action against a city that is raised, served, and then destroyed.
--
-- The process is killed on every path out, including an exception: a test run
-- that leaves cities listening would make the next run's port search fail for
-- a reason that has nothing to do with the product.
withGround :: Door -> (Ground -> IO a) -> IO a
withGround door act =
  withSystemTempDirectory "sprawling-adversary" $ \root -> do
    let city = root </> "city"
    Door.raise door city
    attempt city (4 :: Int)
  where
    attempt _ 0 =
      fail
        "no port between 47100 and 47115 could serve a city; this is the \
        \machine's, not the product's"
    attempt city left = do
      port <- claim
      raised <-
        bracket (Door.serve door city port) hangUp (served city port)
          `onException` release port
      case raised of
        -- The port is deliberately not handed back. Nothing inside this process
        -- held it, so something outside does, and lending it on would give
        -- every later ground the same obstacle.
        Nothing -> attempt city (left - 1)
        Just value -> value <$ release port
    served city port (handle, said) = do
      answering <- waits door port handle 8
      if answering
        then Just <$> act Ground {groundCity = city, groundPort = port}
        else do
          -- Read only on the failing path: the diagnostics are worth the wait
          -- exactly when there is a failure to explain.
          complained <- said
          unless (occupied complained) (fail ("a city would not serve: " <> complained))
          pure Nothing

-- | Stops a served city and waits for it to be gone.
--
-- Waiting matters on Windows, where a terminated process holds its port for as
-- long as it is unreaped, and the next `withGround` would then find it busy.
hangUp :: (ProcessHandle, IO String) -> IO ()
hangUp (handle, _) = terminateProcess handle `finally` (() <$ waitForProcess handle)

-- | Whether a city that would not serve was refused the port itself.
occupied :: String -> Bool
occupied said = any (`isInfixOf` said) ["address in use", "Only one usage", "10048"]

-- | Polls the city through its own client until it answers, or gives up.
--
-- The probe is `city_view` rather than a socket connect: what this adversary
-- needs to know is that the door works, and a port that accepts TCP before the
-- city can answer would start a trace against a city that is not ready.
--
-- Every try costs a process, and a try against a port nothing is listening on
-- costs 2.1 s measured rather than the 0.3 s an answered one costs, so the try
-- count and the port list multiply into the price of a *failure*. Three things
-- hold that price down. A city gets a moment to bind before it is asked
-- anything, because paying 0.4 s of sleep is cheaper than paying 2.1 s for a
-- refused connection. A city that has already exited is never polled again,
-- which is the whole cost of a busy port. And eight tries bound what is left.
waits :: Door -> Port -> ProcessHandle -> Int -> IO Bool
waits _ _ _ 0 = pure False
waits door port handle tries = do
  threadDelay 400_000
  stopped <- getProcessExitCode handle
  case stopped of
    -- A city that has exited will not start answering, and every further poll
    -- would be spent proving that against a port nobody holds.
    Just _ -> pure False
    Nothing -> do
      answered <- Door.ask door port Door.CityView
      case answered of
        Accepted _ -> pure True
        _ -> waits door port handle (tries - 1)

-- | Everything the ledger holds, as bytes.
--
-- Sorted, so that a property about the history does not accidentally depend on
-- the order a directory happened to be walked in.
stored :: Ground -> IO [(FilePath, ByteString.ByteString)]
stored ground = sort <$> walk (ledgerOf ground)
  where
    walk directory = do
      entries <- listDirectory directory
      fmap concat . forM entries $ \entry -> do
        let path = directory </> entry
        isDirectory <- doesDirectoryExist path
        if isDirectory
          then walk path
          else do
            bytes <- ByteString.readFile path
            pure [(entry, bytes)]

-- | Every path a city holds outside its own reserved subtree, sorted.
--
-- The reserved subtree is excluded because it is the city's own account: the
-- ledger and the content store grow whenever anything happens, and a property
-- about "what this command left behind" means the files a person would see.
tree :: Ground -> IO [FilePath]
tree ground = sort <$> walk "" (groundCity ground)
  where
    walk prefix directory = do
      entries <- listDirectory directory
      fmap concat . forM entries $ \entry ->
        if entry == ".sprawling"
          then pure []
          else do
            let path = directory </> entry
                shown = if null prefix then entry else prefix </> entry
            isDirectory <- doesDirectoryExist path
            if isDirectory
              then (shown :) <$> walk shown path
              else pure [shown]

-- | Flips one bit inside the oldest record a ledger holds, the way damage or a
-- hostile disk would.
--
-- Not the end: a damaged tail is a case the product handles on purpose
-- (`memory::jsonl` recovers it), and hitting it would test recovery while
-- claiming to test detection.
--
-- Not the middle of the file either, which is what this did first. A city
-- writes records of its own accord — it noticed it had no provider while this
-- was choosing a byte — so the file's midpoint landed on a chain hash one run
-- and on a record's kind the next, and one test produced two different
-- refusals. The first line is written by `init` before anything else can
-- happen, so it is the one position on this disk that does not depend on
-- timing, and a test that names a code has to hit the same byte every time.
corrupt :: Ground -> IO ()
corrupt ground = do
  segments <- filter (isSuffixOf ".jsonl" . fst) <$> stored ground
  case segments of
    [] -> fail "the city is holding no history to corrupt"
    ((name, bytes) : _) -> do
      let path = ledgerOf ground </> name
          newline = 10
          at = ByteString.length (ByteString.takeWhile (/= newline) bytes) `div` 2
      case ByteString.splitAt at bytes of
        (before, after) -> case ByteString.uncons after of
          Nothing -> fail "the history is too short to corrupt"
          Just (byte, rest) ->
            ByteString.writeFile path (before <> ByteString.cons (byte + 1) rest)

-- | Chops the end off the newest segment, the way a power cut does.
--
-- The tail deliberately, and only here. `memory::jsonl` recovers a torn last
-- line on purpose, so this is the one hostile action that asks whether recovery
-- happens rather than whether detection does. Keeping it a separate verb from
-- 'corrupt' is what stops either from being credited with the other's evidence:
-- a test that tore the tail and then claimed the history was defended would be
-- reporting a supported path as a caught attack.
tear :: Ground -> IO ()
tear ground = do
  segments <- filter (isSuffixOf ".jsonl" . fst) <$> stored ground
  case reverse segments of
    [] -> fail "the city is holding no history to tear"
    ((name, bytes) : _) -> do
      let path = ledgerOf ground </> name
          kept = ByteString.length bytes - 20
      if kept <= 0
        then fail "the history is too short to tear"
        else ByteString.writeFile path (ByteString.take kept bytes)

-- | Writes the oldest record a second time, the way a careless copy does.
--
-- Inside the file rather than at its end, so this is detection and not the
-- recovery 'tear' exercises. A repeated record carries a sequence number and a
-- previous-hash that already belong to the line above it, so a reader that
-- accepted it would be reading a history two different sets of facts can
-- explain.
duplicate :: Ground -> IO ()
duplicate ground = do
  segments <- filter (isSuffixOf ".jsonl" . fst) <$> stored ground
  case segments of
    [] -> fail "the city is holding no history to repeat"
    ((name, bytes) : _) -> do
      let path = ledgerOf ground </> name
          newline = 10
          (oldest, rest) = ByteString.break (== newline) bytes
      if ByteString.null rest
        then fail "the history holds no finished record to repeat"
        else ByteString.writeFile path (oldest <> ByteString.cons newline (oldest <> rest))
