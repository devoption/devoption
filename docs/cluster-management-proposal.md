# Centrally managed clusters and application instances

Status: design direction for [#461](https://github.com/devoption/devoption/issues/461).
[#463](https://github.com/devoption/devoption/issues/463) tracks a narrower
[managed cluster CLI](managed-clusters.md) using prepared Linux nodes and
host-pinned SSH. The following sections describe the broader proposal,
including capabilities that remain unimplemented.

## Current boundary

The [architecture](architecture.md) describes implemented behavior. The browser
controller invokes a fixed local privileged broker; its deployment reviews in
`crates/devoption-server/src/server/projects/deployment.rs` are keyed by project.
`scripts/deployment_broker.py` verifies local configuration, artifact bytes,
runtime fingerprints and approved intent. [Independent app slots](multiple-apps.md)
bind different projects to separate resources on that same host. Those slots do
not provide remote placement or multiple installations of one project. The
separate cluster CLI supports explicit placement and independent instances on
prepared nodes.

## Proposed ownership model

One central controller stores the management catalog and authorizes operations.
Nodes run a constrained agent and the local execution broker. A cluster is a
named placement and resource boundary containing one or more enrolled nodes;
a newly created empty cluster cannot receive a deployment. A one-node cluster
uses the same identity and approval model as a larger cluster. Cluster membership
alone does not establish a scheduler, replicated storage or high availability.

| Object | Identity and ownership |
| --- | --- |
| Controller | Owns cluster catalog, enrollment policy, approvals and audit history. |
| Cluster | Stable ID independent of its display name; owns node memberships and application instances. |
| Node | Stable enrolled identity, exactly one active cluster membership, membership generation and observed capabilities. |
| Project | Source/build identity; may supply releases to several instances without sharing runtime state. |
| Application instance | Stable ID bound to one project and one cluster; owns its placement, configuration, release selection and running/stopped intent. |
| Resource | Explicit instance owner and cluster binding for database, secret grants, storage, hostname and durable delivery state. |

Several instances can reference the same verified immutable release bytes.
Each receives separate mutable state and grants. Names, source repository and
release digest never confer access to another instance's resources. An optional
preview/pilot label is descriptive; it does not alter placement or authorization.
Node-local unit names, directories, ports and database roles must derive from
validated instance identities and reviewed operator policy, rather than project
ID alone. Cluster, node and instance IDs remain distinct types.

The first placement policy is explicit selection of one eligible node within
the instance's cluster. Adding nodes does not move existing instances. Resources
stay inside that cluster; external services require separately declared and
reviewed bindings. No fallback to the controller or another cluster is implicit.

## Enrollment and control boundary

Cluster creation records a catalog entry; attaching a node requires a separate
owner-authorized enrollment. Initial implementation targets operator-prepared
Linux nodes with the supported runtime. Cloud provisioning and paid-service
activation remain separate work.

Proposed enrollment uses a short-lived, single-use invitation bound to the
controller, cluster and expected node identity. The operator independently
verifies the controller trust anchor and node fingerprint before accepting it.
Enrollment must reject duplicate identities, expired/replayed invitations and
cross-cluster substitutions. Credential issuance, custody, rotation and recovery
need a reviewed implementation before enrollment can be enabled; this document
does not request or create credentials.

Use mutually authenticated TLS for agent/control transport. Prefer outbound
node connections to the controller so management does not require exposing a
remote shell or privileged broker. Bind the authenticated certificate identity
to the enrolled node and membership generation. Existing administrative mTLS,
owner sessions, Origin/CSRF guards and MCP permission boundaries remain in force.
An agent certificate never grants browser access or authority over another node.
The first version retains owner-only mutation authority; delegated customer or
cluster administration requires separate authorization work.

The agent accepts a versioned, bounded command protocol, not arbitrary commands,
shell fragments, filesystem paths or systemd unit names from a browser. Every
operation binds controller, cluster, node, membership generation, instance,
expected state generation, operation ID, expiry and exact reviewed inputs.
The node independently verifies those bindings against local policy before the
fixed broker acts. Artifact transfer is bounded and checksum/provenance verified;
availability of a blob is never deployment permission. Node credentials cannot
fetch arbitrary catalog entries, source, releases or secrets.

Durable operation receipts make retries idempotent across connection loss and
agent restart. Reusing an operation ID with different inputs must fail. A lost
response means unknown outcome until reconciliation; it must not authorize a
second deployment. Controller and node audit records contain identities,
generations and outcomes without secret values or enrollment tokens.

Revocation rejects new connections and operations and invalidates pending
approvals. It cannot prove that an unreachable machine stopped its workloads or
deleted its data. Report that distinction explicitly. Removing an identity from
the controller is never evidence of remote cleanup.

## Release approval and resource lifecycle

Builds may initially stay on the controller under the existing build policy.
Remote execution needs a new artifact/provenance handoff; current releases bind
local storage, runtime and project capabilities and cannot simply be copied and
declared deployable elsewhere. Keep source/build provenance distinct from the
destination-specific deployment manifest and validate both on the node.

Deployment review shows project, instance, cluster, exact node, release digest,
hostname, resource grants and resource/runtime policy. Approval binds all of
these, the instance state generation and node membership generation. Changes
invalidate the review. Validate freshness and authorization both when accepting
the request centrally and when executing it remotely. Keep reviews expiring and
single-use; controller restart does not revive pending browser approvals.

Start, update, stop, rollback and logs target one instance. Rollback selects
verified saved code and compatible grants; it does not rewind database content
or undo migrations. Fresh instances receive separately prepared databases,
storage and secret bindings. Sharing a release never clones a database, session
key or integration credential. Secret delivery must be scoped to the selected
node/instance and retained outside guests except through existing supported
capability mechanisms.

Retiring an instance first requires confirmed stop and an explicit retention
decision. Catalog deletion must not recursively delete databases, backups,
volumes or hostnames. Moving an instance or reassigning a node requires a
separate reviewed drain/backup/restore process. Refuse reassignment while it has
active or unresolved owned resources. Never enroll an occupied host by
overwriting existing installation state.

## Disconnection, observations and recovery

Separate desired state, acknowledged operation and fresh runtime observation.
Show unavailable, stale or unknown when the node cannot be observed; silence
does not mean healthy, stopped or empty. Bound health/log collection and include
node identity, observation sequence and age so delayed reports cannot overwrite
newer evidence or another node's state.

An isolated node may continue its last approved workload. It accepts no new
central mutation without fresh authorization; revocation cannot be enforced
remotely while disconnected. The controller must not launch a replacement on
another node merely because a heartbeat expired. Stateful failover requires a
separate fencing and storage design to avoid two writers.

After restart, a node reconciles durable receipts and last approved intent
against the installed runtime, artifact bytes and resource grants. Preserve the
existing stopped-intent rule and bounded recovery policy. Controller restart
reconciles observations and receipts without treating queued work as permission
to execute. Changed destination fingerprints require review under the applicable
recovery policy. No automatic relocation, distributed consensus, live migration,
cross-node database replication or hostile tenant isolation is claimed.

## Delivery sequence and validation gates

Each row is a future bounded implementation issue, opened before coding. This
proposal does not mark these capabilities complete.

| Slice | Deliverable | Required evidence before completion |
| --- | --- | --- |
| 1. Catalog and management | Durable cluster/node/instance identities, owner-only cluster creation and instance associations, initially unenrolled node records. No remote execution. | Restart persistence; strict IDs; cross-cluster association refusal; concurrent updates; unauthenticated/member mutation rejection; no private data in observations. |
| 2. Node enrollment and observation | Opt-in agent, reviewed credential lifecycle, explicit attach/revoke and read-only bounded status through authenticated transport. | Disposable controller plus two nodes; identity substitution, expiry, replay, revocation, stale reports, reconnect and restart failures; no shell or workload execution endpoint. |
| 3. Instance-scoped local execution | Broker/configuration/resource paths keyed by instance, with existing project provenance and explicit per-instance grants. | Two instances from one project; independent data, hostname, secrets, stop/update/rollback; wrong-instance refusal; legacy preservation and running/stopped reboot coverage. |
| 4. Reviewed remote deployment | Verified artifact transfer, destination-bound review, durable command receipts and explicit node placement. | Two clusters including a one-node cluster; deploy two instances of one release; tamper/cross-cluster/replay rejection; timeout after successful execution; independent stop/rollback and controller/node reboots. |
| 5. Resource and node retirement | Reviewed resource preparation, backup/restore/retention, drain and guarded reassignment, using only supported capabilities. | Occupied-host refusal before mutation; state/backup preservation; interrupted drain/recovery; unreachable-node deletion refusal; no credentials copied between instances. |

Catalog work should stay within the Rust monorepo and existing domain/delivery
boundaries; no empty agent crate is needed for planning. The existing local path
must keep working without cluster configuration. A later explicit migration can
map it to a local cluster and instance records after backup and compatibility
validation; browsing the new feature must never rewrite local deployment state.

The current CLI has disposable OpenSSH and systemd evidence for two instances
on one node. A pilot on separate hosts is needed for cross-host networking,
public ingress and full multi-node reboot acceptance. Existing local deployment
checks remain regression gates.
No production rollout, cloud account integration, DNS change or workload launch
is included in this proposal.
