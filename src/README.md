Open two terminals, so that they act as two node.

RUST_LOG=info cargo run
This starts the client locally.

ls p shows the connected peers.
ls c prints the local chain. Can be used to view the genesisi block

create b hello
This created a block, which on broadcasting can be view on the other node via, ls c

Start a third node.  It should automatically get this updated chain because it’s longer than its own (only the genesis block).
After sending the init event, requested the second node’s chain.

create b alsoworks