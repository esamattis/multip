#!/usr/bin/env bats

cmd=./target/x86_64-unknown-linux-musl/debug/multip

@test "can run single command" {
  run $cmd "test: echo test"
  [ "$status" -eq 0 ]
  [[ $output == *"[test] test"* ]]
}

@test "can run multiple commands" {
  run $cmd "first: echo 1" "second: echo 2"
  [ "$status" -eq 0 ]
  [[ $output == *"[first] 1"* ]]
  [[ $output == *"[second] 2"* ]]
}