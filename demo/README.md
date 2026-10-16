# CDF GPT demo (`cdf-gpt`)

Stateless Rust CLI for the **GPT-based CDF v1** layout
(`[design/README.md](../design/README.md)`).

## State machine

```text
S0  raw image / device (no CDF)
     │  create [--force] --profile … --key …
     ▼
S1  sealed CDF (GPT + Contract MAC)
     │  open --key …
     ▼
S2  mapped (/dev/mapper/…); teardown commands printed; disk unchanged
```


| Command  | Transition | Notes                                       |
| -------- | ---------- | ------------------------------------------- |
| `create` | S0 → S1    | Only initialize path; full seal in one shot |
| `open`   | S1 → S2    | Verify + map only; **never** formats        |
| `info`   | S0/S1      | Read-only inspection                        |


`create` on an existing CDF volume **refuses** unless `--force`.

## Requirements

- Linux, **root** (`losetup`, `cryptsetup`, `veritysetup`, device-mapper)
- `sgdisk`, `cryptsetup`, `veritysetup`, `losetup`, `blockdev`, `dd`
- `cryptsetup` **2.5.0 or newer** for both `create` and `open` with `crypt+integrity`.
- Rust (`cargo`)

```bash
cd demo
cargo build --release
sudo ./target/release/cdf-gpt --help
```

If `cryptsetup` fails without diagnostics, set `CDF_CRYPTSETUP_DEBUG=1`
when running `cdf-gpt`. This passes `--debug` to the subprocess and includes
its log in the reported error.

## Disk argument

First positional argument is always the disk:


| Kind                 | Example      | Behaviour                                                                          |
| -------------------- | ------------ | ---------------------------------------------------------------------------------- |
| New or existing file | `./disk.img` | `losetup --partscan` when needed; `--size` required if the file does not exist yet |
| Block device         | `/dev/sdb`   | Used as-is; capacity from the device (**no** `--size`)                             |




## Key material

`--key` / `--key-file` supply `K_root` (never stored on the image).

Optional `--key-locator <uri>` writes a non-secret `Contract.key_locator` hint
(e.g. `kbs:///default/cdf/orders-db`). Deployments (CSI / CDH) override it when
they supply their own locator.

HKDF-SHA-256 (design §6.1):

```text
K_mac  = HKDF(K_root, salt=volume_uuid, info="cdf-v1/meta-mac")
K_luks = HKDF(K_root, salt=volume_uuid, info="cdf-v1/luks")
```



## Profiles (`--profile`)

One shared `create`; profile selects the pipeline:


| Profile                 | Access | `create` extras                                                                 |
| ----------------------- | ------ | ------------------------------------------------------------------------------- |
| `plain` (only for test) | rw     | —                                                                               |
| `crypt+integrity`       | rw     | LUKS2 + `dm-integrity`, detached header                                         |
| `verity`                | ro     | **requires** `--from-image` (content frozen, then hash tree + seal)             |
| `verity+crypt`          | ro     | **requires** `--from-image` (LUKS populate, then verity over ciphertext + seal) |




## Examples



### RW confidential volume

```bash
sudo ./target/release/cdf-gpt ./disk-rw.img create \
  --profile crypt+integrity --size 256M --key 'demo-secret'

# optional `--key` parameter to check metadata HMAC
sudo ./target/release/cdf-gpt ./disk-rw.img info --key 'demo-secret'

sudo ./target/release/cdf-gpt ./disk-rw.img open \
  --key 'demo-secret' --mapper cdf-data
# write via /dev/mapper/cdf-data, then run the printed teardown commands
```

Re-format an existing CDF disk:

```bash
sudo ./target/release/cdf-gpt ./disk-rw.img create \
  --profile crypt+integrity --key 'demo-secret' --force
```



### RO verity volume

Content must be final **before** seal:

```bash
# prepare a small payload image (must fit in P_data)
dd if=/dev/urandom of=./payload.img bs=1M count=32 status=none

sudo ./target/release/cdf-gpt ./disk-ro.img create \
  --profile verity --size 256M --key 'demo-secret' \
  --from-image ./payload.img

sudo ./target/release/cdf-gpt ./disk-ro.img open \
  --key 'demo-secret' --mapper cdf-verity
```



### Existing block device

```bash
sudo ./target/release/cdf-gpt /dev/sdb create \
  --profile crypt+integrity --key 'demo-secret'
```



## Layout reminder

```text
GPT:  P_meta  |  [P_verity]  |  P_data
```

Type UUIDs and Superblock/Contract details: `[design/README.md](../design/README.md)`.