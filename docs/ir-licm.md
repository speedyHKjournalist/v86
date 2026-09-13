# IR-11：有界的纯 SSA 循环不变量外提

本文件补充 `ir-progress.md` 在提交
`372ccdc42cc8cb66c61c283295ab7ab673b73f2f` 时的进度快照。
**IR-11 仅部分实现，IR-00～IR-14 整体仍未完成。** 本次没有修改生产 ISA
覆盖门禁、默认后端、CPU 状态布局、缓存 ABI 或 legacy 路径。

## 实现与接入

`src/rust/ir/passes/licm.rs` 提供 `run(region, work_limit)`：

- 用支配关系识别自然循环，合并同一 header 的所有回边；按内层到外层处理。
- 要求已有唯一、无条件进入循环头的 preheader；循环头不能同时是外部入口。
  不创建块或边，不尝试改造不可约控制流。块的 arena 编号不作为支配顺序。
- 仅移动显式白名单中的全定义纯 SSA 计算：整数运算、位操作、类型转换，以及纯
  SIMD 值运算。依赖必须在 preheader 可用，或已先行外提。
- CPU 状态读取、guest load/store、地址和权限检查、helper、RMW、poll，以及带
  state/commit/fault 策略的指令均保持原位。`!Op::ordered()` 不等于可安全推测执行。
- 固定 arena 上限限制输入规模，工作计数限制循环发现、候选和操作数访问。使用
  克隆事务，前后执行 verifier；失败或耗尽预算不修改调用者的 IR。
- 保持值/指令 ID、StateMaps、CFG 边、effect 链、guest 计数和预算检查位置。
  不改变浮点精度或异常语义。

`passes::run_tier2` 在现有 CFG/phi/常量/GVN/DCE 管线之后执行 LICM。
三个公开编译 API 通过共享 `compile_inner` 接入，仅当 `optimize=true` 且
请求为 `Tier::Two` 时启用；Tier 1 和关闭优化保持原路径，零轮优化不执行外提。
`CompiledArtifact.passes` 增加 `loops`、`hoisted`、`licm_work` 统计。
`hoisted` 是移动次数，同一指令跨越两层循环可计两次，不是性能提升百分比。

## 验证入口

```sh
# 已安装 Rust、Node、make；从任意目录调用均可。
tests/ir/run-licm.sh

# 完整 Rust 和已有的发射器执行矩阵。
RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/run.mjs
node tests/rust/verify-wasmgen-dummy-output.js

# 此额外 decoder oracle 需要 NASM 包提供的 ndisasm。
node tests/ir/decode/oracle.mjs

# 实验 CPU Wasm，不含测试 hooks；需 wasm32 target 和 clang。
make build/v86-ir-runtime.wasm
```

新增 13 项 Rust 测试覆盖依赖顺序、循环携带值、嵌套/自循环、多回边、不可约和
多入口拒绝、条件 preheader 拒绝、预算事务回滚、非法输入、state/poll 保持、
真实 guest load/检查/effect 指令保持、Tier 开关及 SIMD 值链。

独立 JavaScript oracle 执行 9,072 次整数 Wasm 对照和 3,072 次 SIMD 对照，检查
零次循环、溢出、完整 GPR/FLAGS、精确冷退出 PC、计数和无效入口。
SIMD 用例使用明确的 CPU-shaped ABI stub，验证发射代码与状态物化；
**这些 stub 不替代完整 CPU/MMU/Windows XP 验收。**

新增 `IR core` CI 串行执行生成表检查、完整 Rust 测试、Wasm oracle、NDISASM
边界对照和实验 CPU Wasm 构建，保留诊断日志；不把生产覆盖门禁改为通过。

## 仍未完成

本次没有实现 proof-based 访存复用、load/store forwarding、自动创建 preheader、
归纳变量和其它循环/SIMD 优化，也未补齐剩余 ISA、MIR 优化体系或全部链接版本管理。
Windows XP、目标游戏、完整系统差分和性能验收需要后续独立证据。
没有依据此次测试切换默认 IR 后端，也没有删除 legacy。
