# Manage clusters and application instances

DevOption can manage prepared Linux nodes from one controller host through
`scripts/cluster_control.py`. A cluster can start with one node and grow by
attaching more. An instance is a specific deployment of a source project on
one node. Two instances can use the same saved release while owning separate
app slots, hostnames, process state, and optional database grants.

Run the controller CLI from an authorized shell on the controller host. Cluster
management has no browser or MCP endpoints. The controller stores membership,
placements, reviews and receipts in its own private catalog. It does not move
apps or change existing local deployment state. See the
[proposal](cluster-management-proposal.md) for the longer-term design.

## Prerequisites

Use an existing controller that can build and save a release into a
[local project slot](multiple-apps.md). A source release need not be running.
The CLI exports its verified component and build provenance through a separate
root-owned adapter. It never copies the source slot's database policy, data,
environment secrets or integration credentials. The existing deployment broker
executable and configuration stay in place on that source controller.

Prepare each destination as a dedicated node with the same compatible Linux
broker and runtime as the controller. Follow the
[installation requirements](web-app-deployments.md) and the
[explicit multi-app slot schema](multiple-apps.md). Leave the local platform's
app deployment option disabled on the managed node. Its ordinary controller
must not try to manage the remote instance slots. The node expects one to eight
explicit slots, and it rejects a slot that contains unmanaged deployment intent
or an active worker without saved intent. Use newly prepared slots and state
directories. Do not repurpose an existing local slot in place.

For each destination, allocate a different 32-character lowercase hexadecimal
instance ID. Put it in the slot's `project` field and matching ingress
`project` field. The existing broker uses this slot ID for its systemd units,
listener, paths and records grant. Use different ports and hostnames on the
node. If a slot needs PostgreSQL records, prepare its own database, database
identity, private socket/grant and backup policy before staging the release.
Keep those resources separate between instances using the
[record grant instructions](app-records.md), including a distinct non-login OS
identity for each grant on a node. The review shows the grant digest and
read/job capabilities. Host-mediated HTTPS secrets are currently unsupported
on managed nodes. Builds that require that integration are outside this profile.

Each node's operator installs and verifies its own routing, TLS and DNS. A
verified worker response is a local observation. Verify public reachability
independently. The CLI does not change DNS, create hosts, or provision databases
or credentials.

## Install the controller and node adapter

These files are available in the repository and preview bundle helpers. From a
reviewed checkout, install the adapter alongside the root-owned local broker
on the controller and every node:

```sh
sudo install -o root -g root -m 0755 scripts/cluster_broker.py \
  /usr/local/libexec/devoption-cluster-broker
```

On the controller, the operator running the CLI uses the UID authorized in
`deployment.toml` to export existing saved releases. If required, grant it only
`/usr/local/libexec/devoption-cluster-broker export *` via a validated sudoers
entry. The adapter validates exactly one project and release digest and never
exports credential material. Keep `/etc/devoption/cluster-node.toml` absent on
the controller. Use a persistent private directory and protect its parents.
Back up its catalog and control key separately. The CLI accepts an existing
private Ed25519 SSH identity as a separate file. Keep it outside the catalog.

On each node configure a dedicated SSH control user with the UID selected in
that node's `deployment.toml`. Use a root-owned authorized key file and allow
only the prepared controller's public key. An example entry is:

```text
restrict,command="/usr/bin/sudo -n /usr/local/libexec/devoption-cluster-broker cluster-node" ssh-ed25519 PUBLIC_KEY
```

Use a dedicated SSH account with no other login methods or authorized keys and
no access to the ordinary broker through sudo. Disable forwarding for that
account in the server policy. Grant it exactly
`/usr/local/libexec/devoption-cluster-broker cluster-node` through sudoers.
The Unix account may need a login shell to run the SSH forced command. The
forced command must be enforced server-side. The controller pins the Ed25519
host public key and ignores ambient SSH configuration and agent keys. Obtain
and verify the public host key through a trusted channel before attaching a node.
Restrict the node's SSH listener to the management network.

## Create and attach

In these examples, `cluster-control` refers to
`python3 /path/to/scripts/cluster_control.py`. Put global options before the
subcommand. Capture the cluster ID printed by `create-cluster`. Prepare the
node ID, membership epoch and instance IDs during setup. They are identifiers,
not passwords.

```sh
cluster-control --state "$CATALOG" create-cluster "Example cluster"
```

Install node membership as root-owned mode 0600 at
`/etc/devoption/cluster-node.toml` before attaching:

```toml
cluster = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
node = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
epoch = "cccccccccccccccccccccccccccccccc"

[instances]
"dddddddddddddddddddddddddddddddd" = "11111111111111111111111111111111"
"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee" = "11111111111111111111111111111111"
```

The keys in `instances` are that node's exact deployment slots, and the
values are the controller source project IDs. The catalog stores each
node once and refuses membership in a second cluster. Instance names are unique
within a cluster. Hostnames are unique across the controller catalog. Attach
another node by preparing its own identities and repeating this procedure.

```sh
cluster-control --state "$CATALOG" --identity "$CONTROL_KEY" \
  attach-node "$CLUSTER" "$NODE" "$EPOCH" "$NODE_HOST" 22 "$NODE_USER" "$HOST_PUBLIC_KEY"
cluster-control --state "$CATALOG" --identity "$CONTROL_KEY" \
  attach-instance "$CLUSTER" "$NODE" "$INSTANCE" "$PROJECT" "Preview"
cluster-control --state "$CATALOG" inventory
```

## Stage, deploy, observe and recover

Stage a release from the controller's saved releases. Staging verifies the
component framing and digest and writes an immutable destination release. It
does not start an app and does not copy the source runtime grants. Use the
destination release digest printed by `stage` for a deployment review. The
review includes the exact target, source and destination release, hostname,
record grant, expected health body, current state generation and configuration
fingerprint. Approvals expire after five minutes. Review and approve separately.

```sh
cluster-control --state "$CATALOG" --identity "$CONTROL_KEY" \
  stage "$INSTANCE" "$SOURCE_RELEASE"
cluster-control --state "$CATALOG" --identity "$CONTROL_KEY" \
  review-start "$INSTANCE" "$DESTINATION_RELEASE" "ok"
cluster-control --state "$CATALOG" --identity "$CONTROL_KEY" approve "$OPERATION"
cluster-control --state "$CATALOG" --identity "$CONTROL_KEY" status "$INSTANCE"
```

Add `--logs` to `status` to read the selected worker's bounded log tail.
`node-status` performs an immediate membership probe. Unavailable status
does not assert that a workload stopped. To stop a workload, use `review-stop`
and approve its operation. Use `operations INSTANCE` to find recorded review and
operation IDs. To roll back, review and approve a previously
staged destination release. Rolling back code does not reverse migrations or
restore data. Resource and hostname bindings are part of the review and must
still be valid.

The CLI consumes an approval before sending an operation. If the response is
lost, it reports an unknown outcome. Use `reconcile OPERATION` and
`status INSTANCE` to read the durable node receipt and current observation.
A failed receipt also warrants inspection. The broker can stop a failed
candidate during startup. An unknown or failed result does not automatically
retry or launch an instance elsewhere.

To recover after boot, an operator can configure a local systemd oneshot to
run `/usr/bin/sudo -n /usr/local/libexec/devoption-cluster-broker cluster-recover`
as the configured control UID, after the node's network and required resources
are ready. Start with the [recovery unit example](../examples/systemd/devoption-cluster-recover.service)
and confirm its user and ordering. Grant that exact command separately with sudoers. SSH keys still
force only `cluster-node`. Recovery restarts only previously healthy approved
running intent whose runtime and bindings still match. Stopped intent stays
stopped. Review and redeploy if a changed destination fingerprint prevents
recovery. A failure for one slot does not stop recovery of a sibling.

## Limits and maintenance

- This preview uses explicit placement and root-prepared resources. No automatic
  cloud provisioning, placement, failover, replication, migration, destructive
  cleanup or hostile tenant isolation is claimed.
- To revoke management, run `disable-node NODE` on the controller and remove
  the controller key through the node's trusted administrative path. The CLI
  invalidates its pending reviews. Controller-side disable never claims to
  remove remote access, stop workloads or erase data. Reassignment, reenabling
  retired identities, and automated SSH host key rotation are not supported.
- Back up the controller catalog, each instance's data and each node's release
  and intent files separately. The normal broker's limit of 32 releases per
  slot applies. Node receipts are retained, with a limit of 4,096 per slot.
  The controller catalog is capped at 2 MiB and is serialized per operation.
  Inspect capacity before running out. No automatic history deletion or
  membership reassignment is provided.
- The local tests exercise two logical clusters with independent files and
  failure cases. The disposable Linux acceptance path uses real OpenSSH and
  systemd to check two instances, update, rollback, stop and running/stopped
  recovery on one node. It does not establish cross-host networking, full
  multi-node reboot acceptance or public ingress.
