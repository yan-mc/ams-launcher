# 更新公钥（只读）

- `updates-public.pem`：内置到启动器的 Ed25519 **公钥**，用于验证 `ams-components.json` / 更新清单签名。
- 当前为 dev/demo 公钥（与 `ams-release/examples/keys/public.pem` 配对）；**私钥不在本目录、不入库**。
- 正式发布前替换为生产密钥对，并把私钥只放 CI Secret/离线保管。
