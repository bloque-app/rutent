# Recovery from total loss of the VOS cluster (`T-010-08`)

> AGPL-3.0-or-later with the rest of this package (`vos-licence-v0`).

`DEC-26` asks for a total-loss recovery story before v0 ships on VOS: scheduled halted exports with their downtime and
their RPO written down, or upstream `vos node restore`. This is the first. Total loss means every node of the cluster
and its disks are gone. Losing one node, or two of three, is not this: the replicas cover it (`R-01`, `R-02`).

## What a halted export is

The `Host` contract's `halt`, `export` and `resume` (host contract 2.0.0). `halt` refuses new submits, `export` reads
every released record and every refusal out as one stream that ends with its counts, and `import` onto fresh nodes
rebuilds the journals from that stream. A submit that was acknowledged
before the halt is in the stream: that is `NFR-6` and `INV-7`.

## The schedule

1. Core halts the host, exports to the end of the stream, writes the stream to storage that is not on the cluster's
   hosts, checks that the stream's end entry counts the records and refusals it carries, and resumes the host. The stream is the backup.
2. Keep the last two streams at least, so that a stream found bad at restore time leaves the one before it.
3. The interval is chosen by `F-030`. **The RPO is that interval**: a submit acknowledged after the last export is
   lost in a total loss, and everything before it is kept. A shorter interval costs more halts.

What a halt costs, from `R-08` (one noisy runner disk, in one job, not production):

| Records in the journal | Halted for the export, one node | Halted for the export, three nodes |
|---|---|---|
| 1,344 | 0.04 s | 0.07 s |
| 10,304 | 0.33 s | 0.51 s |
| 100,352 | 3.60 s | 4.85 s |

A submit arriving during the halt is refused, not recorded, so its sender retries it. The halt grows with the
history the export carries, which `T-010-07`'s archive bounds.

## The restore

1. Build three fresh nodes with `build.sh` and start them with an empty data directory each. Run
   `host-check.sh` on them first (`G-37`): a host that fails it must not take the import.
2. Take the newest stream whose end counts match its entries, and import it through `Host::import`, in pages of at
   most 8 entries or 48 KiB (`VosHost::import_from`). The host refuses submits until the import has finished.
3. Read each order's journal back and compare its last sequence number with what the senders were last told, for the
   orders you can still ask about. Resume, and let the senders retry: dedup on the record id makes a retry safe.

The downtime is the import. From `R-08` (halt to export, then import to ready, total):

| Records | One node | Three nodes |
|---|---|---|
| 1,344 | 1.3 s (import 1.21 s) | 3.9 s (import 3.88 s) |
| 10,304 | 6.3 s (import 6.00 s) | 10.1 s (import 9.60 s) |
| 100,352 | 59.1 s (import 55.48 s) | 81.8 s (import 76.97 s) |

## The drill

`halted_export_imported_onto_fresh_nodes_keeps_every_record_acknowledged_before_it`, in
`client/tests/vos_node.rs`, runs this on three real VOS nodes in CI (`stm-vos.yml`): it submits two orders, halts,
exports, kills the cluster and deletes its data, imports the stream onto three fresh nodes, and checks that every
acknowledged record is held once at its sequence number and that every journal equals what it was. It is the restore
without the storage step. The drill does not time the restore at 100k records; `F-033` measures `R-08` again.

## Not covered

- Power loss and loss of the host were never run on either candidate (`outcome.md`); the drill's kill removes the
  data, it does not test what a disk holds after a power cut (`G-54`, `T-010-06`).
- Upstream `vos node restore`, the other story `DEC-26` names, is not used here.
- Where the streams are stored and who runs the schedule are `F-030`'s.
