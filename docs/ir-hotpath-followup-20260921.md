# IR 热后继、缺失入口与基准计时修复（2026-09-21）

## 状态和基线

基于 `ir` 的 `6823793e0113d5ae3b2fc0359760612e569b6cf8`（合并 PR #51）。
本文记录上一轮本地补丁的验证结果；2026-09-22 将同一执行代码提交为面向 `ir` 的 PR。
本次提交仅更新交付状态说明，不改变下述已经验证的执行代码和测试。
**完整性能门槛仍未通过；未取得 XP 镜像，没有进行 XP 到桌面的性能复测。**

环境为 Linux x86-64、Rust 1.98.1、Node 24.17.0，使用同一份匹配 PR #51 的 JS。
各性能任务独立 VM、串行交替，本任务未并发运行构建或回归；没有声明物理核绑定，
也没有把容器测量称为用户 Mac 的性能。点估计来自下面预先设定的三轮或五轮。

最终源码未修改 legacy JIT、取指函数或 CPU 指令语义；试过的全局取指内联设置已撤下。
IR 缓存查询位于共同分派器之前，因此仍测量两版核心的 legacy 对照，而不是假定其时间绝对不变。
执行预算、区域大小、热度阈值和融合策略保持基线设置。

## 1. 需要更正的性能证据

原 `boundaries.mjs`、`fixed_work.mjs` 和 `cold_publication.mjs` 在看到 HLT 后，
先 `await vm.stop()` 再读取结束时间。`src/main.js` 的 stop 只是设置 stopping 标记；
已安排的 idle tick 到来才确认停止。本轮原始记录中停止确认经常额外等待约 100 ms。

例如早期校正计时的同类样本：IR 实际观察到 HLT 为 9.349787 ms，随后停止等待
100.283101 ms；legacy 的对应样本为 8.032568 ms 和 99.247044 ms。
共同的大常数会使短程序的吞吐比被拉向 1，旧固定工作量“通过”不能继续证明热态接近 legacy。
旧跨页报告的 34.76% 也包含这一等待，不能称为单纯执行吞吐比。

新增 `timing.mjs`，在观察到 HLT 后、等待停止之前确定 `ms`，独立报告 `stop_wait_ms`；
所有架构状态与退休数断言保留。`timing_test.mjs` 用虚拟时钟验证 0/1/100/1000 ms
停止延迟都不改变执行时间，并保留停止失败的异常。它加入 `ir-cache-tests`。

这**不是**解释 XP 40 对 100 mIPS 的借口。XP 比较器使用显示事件记录的
`milestone.ms` 和 `milestone.instructions`，不是停止后的 result.ms。
XP 的设备等待、时钟轮询以及冷编译仍属于其端到端工作负载；本补丁没有扣掉这些时间。

## 2. 实际修复及安全边界

### 已验证后继的快速交接

`runtime/cache.rs` 将一次区域调用拆成准入与执行阶段。普通出口完成统计时，
在已有 cache guard 下尝试已经观察并验证过的后继，不重新执行整套冷准入管理。
准入与交接函数显式内联，避免拆分阶段引入额外的结构体调用边界。

资格要求包括非饱和 epoch、无待回收记录、前后 owner/index/唯一发布身份匹配、
当前别名仍有效、精确入口上下文、代际，以及所有源（含融合源）的当前 TLB 映射。
目标必须在同一同步 epoch 内已验证源码。实际 `get_phys_eip` 仍执行，
且只有证明 TLB 命中的路径可在 guard 下取指；不允许在这里回调宿主或走页表。

诊断、性能记录、强制完整验证、observer 出口、控制位改变、预算结束、失效或回收
均走保守路径。仍保留初始一次加至多 64 个后继的上限，不使用无界递归。
**这不是任意模块间的 SSA 寄存器保留；普通区域出口仍写回 CPU 状态。**

### 未发布热点的精确缺失提示

增加单个精确 `CpuEntryKey` 缺失提示。只有完整查找确认没有发布的目标、且没有待回收
记录时才记录；后续相同 PC/CS/位宽的缺失可以直接返回普通解释/legacy 分派路径。
它只证明“无可运行 IR”，不授权执行任何代码，不跳过解释器的取指或异常检查。

发布、失效、取消、替换、淘汰等引起维护的路径均清空提示。替代位宽或 CS 的缺失不能
遮蔽另一入口；原始宿主改码仍由解释器读取新字节。诊断模式不使用这条快捷返回，
以保留原有缺失归因。IR 后端仍不偷偷启用 legacy JIT。

### 合并单入口 guard 与初始化

新增 `ir_enter_checked`：精确上下文判定成功后才初始化 REP/previous_ip。
单入口生成代码少一次不透明 Wasm-to-Wasm 导入调用；任何错误键、前缀、位宽、
调用选择器或 legacy 活动帧仍必须无副作用拒绝。共享别名继续使用原独立 guard。

这里的 Rust runtime 也是 Wasm，不把每次跨区域调用错误描述成 Wasm→JavaScript 往返。

### 诊断与可复现接口

| 接口 | 含义 |
| --- | --- |
| `ir_cache_set_warm_chaining(0/1)` | 冷点控制已验证后继交接，非法值或活动帧拒绝 |
| `ir_cache_stat(35)` / `(36)` | 交接累计次数 / 开关 |
| `ir_cache_set_missing_hint(0/1)` | 冷点控制缺失提示，不改变编译策略 |
| `ir_cache_stat(37)` / `(38)` | 缺失提示命中累计数 / 开关 |

XP runner 增加两个计数的分段累计、功能支持标记和候选侧 A/B 参数，正确处理 u32 回绕。
旧核心不支持的新字段在比较表中表示为 null，不以 0 冒充旧策略的真实计数。
`IR_WARM_CHAINING` / `IR_MISSING_HINT` 的显式开关用于新核心的 `xp_boot.mjs` 单独 A/B；
默认三方案 `xp_compare.mjs` 不应向旧基线强行传入其不支持的开关。

## 3. 最终测量（均已剔除停止确认等待）

### 普通跨页边界，五轮

40,000,000 次算术更新、四页、每片段三条算术指令；每轮严格退休
170,000,003 条指令。关闭融合仅为隔离普通边界，其他生产策略不变。

| 中位数 | 基线 IR | 本补丁 IR | 本补丁核心 legacy |
| --- | ---: | ---: | ---: |
| 毫秒 | 2514.470 | 2000.660 | 172.263 |

相对基线 IR 的吞吐变化为 **+25.68%**；
5/5 配对改善。
但本补丁 IR 仍只有同核心 legacy 的 **8.61%**。
这与旧 34.76% 的协议/长度不同，不能直接相减；相同新协议内，补丁确实比基线快。
原始数据 `final/delivery-long.jsonl`。

### 冷编译/发布，五轮

32 个分离页面，每轮严格退休 12,582,978 条指令。

| 中位数 | 基线 IR | 本补丁 IR | 本补丁核心 legacy |
| --- | ---: | ---: | ---: |
| 毫秒 | 313.434 | 272.259 | 138.489 |

IR 吞吐变化为 **+15.12%**；
4/5 配对改善。
IR 退休覆盖反而由 20.52% 变为 19.10%，
不能将其说成覆盖提升或发布策略已解决。结果与减少未发布入口重复查询的目标一致，
但不能从这些总量给每项修改分配独立的加速收益。
原始数据 `final/delivery-cold.jsonl`。

### 已预热固定工作量，三轮、工作量乘五

`IR_FIXED_SCALE=5` 只扩大测量阶段，warmup、CPU 策略和性能门槛不变。
两版核心及各自的 IR/legacy 四个方案交替运行，检查等退休数及最终架构状态，
测量段 IR 退休覆盖至少 95%。这仍包含周期性预算退出和热态运行时，不是单独测某条 ADD。

| 负载 | 基线 IR mIPS | 本补丁 IR mIPS | 本补丁 legacy mIPS | IR/legacy | IR 相对基线 |
| --- | ---: | ---: | ---: | ---: | ---: |
| integer | 990.746 | 1002.461 | 1314.144 | 0.763 | +1.18% |
| ram_rmw | 530.555 | 553.899 | 660.608 | 0.838 | +4.40% |
| indirect_regions | 533.970 | 630.227 | 123.595 | 5.099 | +18.03% |
| sse_register | 654.626 | 644.466 | 832.836 | 0.774 | -1.55% |

几何平均为 1.260，但整数、RAM RMW、SSE 均未达到每项至少 0.9
的原门槛。**完整门槛失败，脚本退出 1。** SSE 相对基线也有小幅回退，没有隐藏。
相同计时协议下，基线同样未通过这些单项；不能恢复旧的停止等待以让门槛重新变绿。
原始数据 `final/delivery-fixed.jsonl`。

## 4. 最终正确性验证

`final/release-regression.log` 对应最终执行源码，脚本退出 0。
production / ir-experimental 的 release Wasm `-D warnings` 检查通过；
原生 250 项通过、2 项显式微基准忽略。

`make ir-entry-tests ir-cache-tests ir-auto-tests ir-live-tests ir-diagnostic-tests
ir-fusion-tests ir-sti-tests ir-helper-reload-tests ir-helper-audit` 通过。
覆盖 debug/release/正式核心的缓存、发布、恢复、共享入口和交接；
新增 12 组交接开关对照、缺失提示开关及原负缓存双路径测试。
原始/通知改码、访存映射、owner 替换、宿主回调、计数回绕和诊断守恒均在相关回归内。
故障注入器识别新旧两个入口函数名，保留原有拒绝/重试抑制断言。

没有声明完整远端 CI、实际浏览器/Worker 全矩阵、跨版本存档或 XP 系统验收通过。
XP runner 新增统计路径仅完成语法检查，没有伪造未运行的 XP 日志。

## 5. 中间失败与限制

中间版本曾出现：暖交接单独无稳定收益、冷态回退约 8.4%、校正计时后热态回退。
后续增加缺失提示、内联准入传递，并撤下全局取指内联。中间日志保留在 `intermediate/`，
其中旧文件仍含 stop 等待，不能代替本节最终数据。`accepted-*.jsonl` 是中间实验名称，
不表示其性能门槛通过；以 summary.pass 和对应退出码为准。

一次旧验证脚本误把 `.wasm` 文件名传给需要前缀的 STI 脚本，产生调用错误；
随后正确重跑。最终整套回归退出 0，不用旧失败脚本的中间输出冒充最终通过。

当前结论是局部性能修复与验收纠错，不是 XP 从 40 提升到 100 mIPS。
代码生成、周期性退出、状态写回、在线编译与发布等待的剩余成本仍存在。

## 6. 应用和复现

在干净工作区基于指定提交建立测试分支，先保存旧 Wasm：

```sh
git switch -c ir-hotpath-followup 6823793e0113d5ae3b2fc0359760612e569b6cf8
git apply --check /path/to/v86-ir-hotpath-6823793e.patch
git apply /path/to/v86-ir-hotpath-6823793e.patch
make build/v86-ir-runtime.wasm build/libv86.mjs
make ir-entry-tests ir-cache-tests ir-auto-tests ir-live-tests ir-diagnostic-tests
```

证据包的 `binaries/baseline-6823793e.wasm` 是本轮真实配对基线；不要用更早的 19989ac5
或 b991dccf 替换它来声称本补丁收益。以下各任务串行、不要与构建同时执行：

```sh
BASE=/path/to/evidence/binaries/baseline-6823793e.wasm
IR_COMPARE_RUNS=3 IR_FIXED_SCALE=5 node tests/ir/performance/fixed_work.mjs build/v86-ir-runtime.wasm "$BASE"
# 上一命令当前会因原性能门槛失败而返回 1；不要吞掉该失败。
IR_COMPARE_RUNS=5 IR_BOUNDARY_UPDATES=40000000 IR_BOUNDARY_BODY=1 IR_BOUNDARY_SHAPE=page_boundaries \
  node tests/ir/performance/boundaries.mjs build/v86-ir-runtime.wasm "$BASE"
IR_COMPARE_RUNS=5 node tests/ir/performance/cold_publication.mjs build/v86-ir-runtime.wasm "$BASE"
IR_BASELINE_WASM="$BASE" node tests/ir/performance/xp_compare.mjs /path/to/windowsxp.img build/ir-hotpath-xp
```

最后一条是待运行的 XP 验收，不是本轮已经完成的测试。现有自动里程碑仍是首次
800×600×32 显示模式，不等于完整桌面空闲。真正桌面验收必须对两方案使用同一定义。

## 指纹

- 基线 Wasm：`b6cdd510dd5d86770963edb3073665bbd3d1400cdd165487ed8edd283604944e`
- 本补丁 Wasm：`b57c7acaf586f9e97a17ba4e9ae255037ec3511df1ba37271e3bd2790ce313de`
- 配对 JS：`3e597e24600dff3e11305835bb6e9648b9273c1a30157961b6f190a25b563947`
