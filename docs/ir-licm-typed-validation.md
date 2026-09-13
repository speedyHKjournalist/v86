# IR-11 增量：i64 / SIMD 恢复验证与无变换路径

本增量基于已合入的 PR #5，基线为
`75a9d125aec9f4c19dcde5c1ddef17b9cb949f86`。保留其 LICM Config、
Tier 2 接入、统计、预算、白名单及原有测试，不替换并行开发成果。
IR-00～IR-14 的完整迁移仍未完成；参见 [实施进度](ir-progress.md)。

## 实现变化

`licm::run` 在输入通过 verifier、完成自然循环发现后，若没有可用循环，
直接返回原有工作统计，不克隆 Region。若扫描后没有移动任何定义，则不
再次验证未改变的副本。真正发生外提时，仍需完整验证才能提交事务。
没有删除入口校验、放宽异常语义、改变外提预算或启用默认 IR 后端。

这是省去无效编译工作的局部改进；尚未测得游戏加载时间或 CPU 跑分收益。

## 新增验证

三个 Rust 测试覆盖混合宽度/向量依赖链、入口专属 CPU 读取的拒绝及无变换
路径。使用块分配顺序与支配顺序不同的循环，外提 i64 和 V128 不变量，
保留循环携带值；在 Wasm 发射前释放 HIR，检验独立 MIR 的生命周期。

`tests/ir/wasm/licm_typed.mjs` 执行 166,656 次有/无优化 CPU-ABI 模块，
在八档预算、零次及多次迭代、整数边界/确定性随机输入、FLAGS 和 CS
组合下，用独立 BigInt 算术与控制流参考核对：

- i64 溢出、低/高 32 位结果及 SIMD 逐 lane 结果；
- 动态退休计数回绕、CS/EIP 回绕、精确预算退出位置；
- FLAGS、lazy flag operand 和全部八个 XMM 的 StateMap 恢复；
- 完整 CPU 状态与保护字在有/无优化路径之间一致。

此 oracle 使用真实生成的 CPU ABI 和受控 JS 回调；它本身不等同于完整
CPU/MMU/操作系统测试。额外的真实 CPU 验证见下节。

完整 Wasm runner 已接入新 oracle。独立复现入口无需 OS 镜像或 NASM：

```sh
tests/ir/licm.sh
```

## 本次实际执行的验证

在上述合并基线加本增量后，重新执行而非复用旧分支结论：

| 项目 | 结果 |
| --- | --- |
| `RUSTFLAGS="-D warnings" cargo test` | 143 passed，0 failed |
| `cargo test ir::passes::licm` | 16 passed，含本增量的 3 项 |
| 新增 i64/V128 CPU-ABI oracle | 166,656 次执行通过 |
| PR #5 原有 LICM Wasm oracle | 21,600 次执行通过 |
| `node tests/ir/wasm/run.mjs` | 完整 standalone runner 通过 |
| `node tests/rust/verify-wasmgen-dummy-output.js` | 通过 |
| `build/v86-ir-test.wasm` | 本地重新构建成功 |
| `build/v86-ir-runtime.wasm` | 本地重新构建成功，无 test-hook 导出 |

重建后逐项执行：

```sh
for suite in cfg loops memory live; do
    node "tests/ir/differential/${suite}.mjs"
done
for suite in live_runtime cache auto backend; do
    node "tests/ir/differential/${suite}.mjs" build/v86-ir-runtime.wasm
done
```

以上均通过，覆盖实际 CPU 差分、真实异常/内存、在线编译、代码捕获与
重新校验、缓存失效/发布、自动升档及公开后端。包括禁用 legacy 编译后
自动执行与升档、SMC、快照恢复和精确退休计数。没有将此子集称为完整
IR/系统/浏览器矩阵。

本地环境使用 Rust 1.98.1 和 Node 22.16.0；IR CPU Wasm 从修改后的源码
构建，未修改的 legacy 参考核心、JS 与 C 对象使用原工作区构建产物。
链接器包装器产生两条空 stdout/stderr 警告，构建成功。
本地缺少 `ndisasm`，解码长度 oracle 未通过本地执行；合并基线 CI 已安装
NASM，远端结果必须以本 PR 对应提交的 Actions 为准。

`make ir-default-gate` 仍明确拒绝默认切换：3,728 个 production forms 为
Pending。此门槛未被绕过。未完成全 ISA、其余 IR-11 优化、完整链接/版本
管理、XP/应用/性能验收或 legacy 退役，也未修改现有实施进度为“完成”。
