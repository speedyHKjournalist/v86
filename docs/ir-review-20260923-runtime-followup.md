# IR 设计与性能追加审查（2026-09-23）

本轮固定基线 `a887278a5fc79ed499c2815a822d73041da287ba`，依据
`v86-ir-implementation-plan.md` 和 `ir-progress.md` 继续复核。开始时工作区干净。
原 release 核心保存在 `build/ir-next-baseline.wasm`；XP 使用已有
`windowsxp_multidisk_C_4G.img`，磁盘写入只进入 RAM overlay。

## 审查结论

实验目录的 Pending=0 只证明每种粗粒度编码有 lowering/helper/明确基线行为，
不能代替完整前缀、模式、权限、故障及操作系统验收。默认迁移门槛保持不变。

| 计划 | 本轮核对与处理 |
|---|---|
| IR-00 | 保留基线、公开配置、真实核心与配对测量；核对性能与诊断口径 |
| IR-01 | 宽索引与 LEB 已有实现和边界执行回归，无重复重写 |
| IR-02 | 共享 decoder 已接入；CR/DR 和 CPUID 的新续执行同步更新捕获边界 |
| IR-03 | owned MIR、SSA、effect、恢复图与独立验证已有实现；重检 Tier 1 的需求分析遗漏 |
| IR-04 | 新增只读系统 helper 续执行，debug 宿主日志继续保留权威退出；通用细粒度状态契约仍有缺口 |
| IR-05 | 整数及 FLAGS 原生实现已有覆盖；补 Tier 1 对精确 lazy backing 的利用 |
| IR-06 | 保留 RAM/MMIO、跨页、代码别名及 RMW 部分完成契约 |
| IR-07 | 补 CPUID、CR/DR 读取续执行；POPF、控制寄存器写入继续保留系统边界；REP 已使用现有有界 CPU 字符串引擎 |
| IR-08 | x87/MMX/SSE 寄存器续执行已有实现，保留 canonical F80 与严格特殊值语义 |
| IR-09 | Tier 1 补 CPU 状态需求与入口等价写回裁剪；不默认运行完整 Tier 2 优化 |
| IR-10 | HIR 中间诊断校验遵循配置，强制管线入口/出口和公开事务校验保留 |
| IR-11 | 地址/访存证明仍限明确支持范围，通用归纳变量和含写入循环的提升未完成 |
| IR-12 | 补过期字节证书的直接链接、入口提示哈希与实际热源调度；保持 owner/generation/全部映射验证 |
| IR-13 | 继续实际 XP、等量退休指令和浏览器回归；按测量判定，不能用静态功能覆盖宣布性能完成 |
| IR-14 | 未通过默认迁移验收；默认后端与旧 emitter 保持现状 |

## 已定位的问题

- Tier 1 没有启用现有 CPU FLAGS demand，也未利用 lowering 已证明的入口等价状态。
  现在按实际恢复需求省略纯值计算，只启用已有入口证书，不额外跑观察点后的全局状态分析。
- 观察点撤销字节证书后，普通后继反复绕回冷入口。现在在已有缓存锁内做完整字节与
  映射验证，成功才直接交接；缺映射、代码陈旧、失效、诊断和预算边界仍走原路径。
- 64 槽入口提示使用低位索引，对齐入口大量冲突。现在混合完整入口 key，容量不变，
  每次仍比较完整 key。故意碰撞、发布可见性、模式与 CS、raw/notified 写入均有回归。
- 解释器记录的只是“发生过解释执行”，调度时却查询跳转后的 PC，错过刚达到阈值的
  源入口。现在保存完整热源 key，在冷点重新核对热度、失败状态与当前 cache tier；
  未达到阈值的访问不再重复进入冷检查。提示不授权执行或发布。
- CPUID、CR/DR 读取原来总退出。正常无观察路径现在重载 GPR/FLAGS 继续；写入仍退出。
  debug CPUID 的日志回调可能任意修改 CPU/代码，必须先撤销证书并保留调用后的权威状态。
- release 中 HIR 在每个 pass 后无条件整图验证，忽略 `ir_verify`。现在强制验证入口与
  最终结果，中间诊断按 Off/Debug/EveryPass；公开变换事务原有检查不减少。

## 后续 FLAGS 与生成代码审查

`ReadFlags` 原来把所有入口算术标志绑在一次 `get_eflags` 上，即使需求分析只保留
CF 或 ZF，也不能消除其余标志的计算。现在拆为六个 `ReadFlag(bit)` 和单独的
`ReadSystemFlags`，继续调用 CPU 原公式；raw backing 与 masked system 有不同 SSA
身份和来源证明。ADD 不导入入口标志 getter，INC/ADC 只导入 CF，JZ 只导入 ZF。
完整 CpuReload ABI 保持不变。对伪造 masked-system→CF 来源增加拒绝裁剪的测试。

Tier 1 共享代码体现在最多包含三个实际观察过的邻近入口；独立编译回退仍要求
原热度阈值，且累计 suffix 字节不能超过原输入预算。这会增加可执行入口覆盖，
也可能增加一次共享尝试或后续升档压力；详见 [侧入口契约](ir-observed-entries.md)。

CPU-only demand 现在保留到 CFG 边：按实际需求筛选 SSA argument/parameter 配对后
重新调度 parallel copies，不能直接删除旧 copy 序列中的某个 move，否则会破坏副本环。
独立 verifier 重算需求与调度，预算失败保守保留完整计划，重新分配 local 后丢弃旧计划。
standalone ABI 保留完整状态；结构化、预算批处理、latch 和 dispatcher 四条发射路径均使用同一 CPU 计划。

IR emitter 按实际引用声明 locals。新增的 I32/I64/V128 fresh-local API 只创建新的
Wasm 声明，不复用 free list，也不生成显式 const-zero/store；函数入口的 Wasm
零初始化提供相同语义。这同时消除未使用的通用 locals、重复入口清零及相关字节码。
原 builder 分配 API 与 legacy emitter 调用保持原行为。实际 Wasm 回归包含先释放
非零同类型临时值、跨 256 个 local 的 LEB 边界、重复调用与 V128 值。

不可变 XP replay corpus 的 2,540 个捕获输入全部编译成功，产物共 34,236,297 字节。
这是一项覆盖及体积记录，不把缺少固定源 SHA 的历史体积当作同基线性能对照。

## 编译器干涉图与配对重放

HIR 和 owned MIR 的 local 分配均按 SSA ID 升序着色；处理当前值时，只有更小
ID 的邻居已获得 local。因此将每条无向干涉边仅存入较大 ID 的行，并将 dense
行长度限制为当前行 ID 之前的前缀。完整逻辑干涉关系、类型、颜色选择顺序与原
工作预算保持不变，私有 `occupied` 查询明确要求升序使用。原 symmetric 表示
保留为测试 oracle；逐边、lower-prefix occupied、完整 Allocation，以及跨类型、
远距离 ID、离开后重新进入 live set 的案例均比较一致。独立 MIR 碰撞验证继续保留。

先前试验的 sparse word-batched 行在 8 指令分配中比 dense 慢约 66%，且保守的
merge 工作量计费使 32/96 指令图触及原预算，已完整回退；没有为了保留该试验
扩大预算。最终采用上述更小的 triangular dense 改动。

使用冻结的 stage4 原生测试程序与 triangular 候选程序，在无其它构建或性能
测试的窗口对同一不可变 XP corpus 做三轮交替重放。每轮两臂均为 2,540 成功、
零失败，总产物 34,236,297 字节，指纹均为 `91d2542c37cd5d37`。原生程序与
corpus 的 SHA-256、六次原始记录及顺序见
[编译重放记录](performance/ir-runtime-followup-20260923/triangular-compiler-replay-summary.json)。

| 编译阶段 | symmetric 中位 ms | triangular 中位 ms | 耗时降低 |
|---|---:|---:|---:|
| 完整编译重放 | 10547.171 | 10420.048 | 1.21% |
| HIR local 分配 | 486.016 | 436.841 | 10.12% |
| owned MIR local 分配 | 188.094 | 175.228 | 6.84% |

同一候选程序中的分配器微基准做七轮交替、每样本 20 次分配；8/32/64 条 INC
另加 DEC/JNZ 的中位耗时分别为 135.9/540.9/1205.7 → 121.1/492.4/1096.4 μs，降低约 9%～11%。
[微基准记录](performance/ir-runtime-followup-20260923/triangular-allocation-microbench.json)
仅覆盖 HIR 图分配。追加改动后的完整原生套件为 288 passed、0 failed、4 ignored。

本项证据只支持**总编译时间降低约 1.2%**，不包含 guest 执行，不能据此声明
XP 启动或 mIPS 提升，也不能替代 XP 与 legacy 的配对验收。

## 验证与测量

完整 stage4 三轮结果见下文；stage5 继续验证，尚不能宣布通过。探索阶段样本不能作为验收：首组旧 IR
29.121 秒 / 62.480 mIPS，新 IR 27.158 秒 / 60.077 mIPS，legacy
19.619 秒 / 127.663 mIPS。终点为首次 800×600×32 显示模式，不是桌面空闲。
两项指标应一起报告，XP 计时设备与忙等导致不同后端的退休指令数不同。


本轮还定位并修复两个测试设施缺口：

- CR/DR 的预期 Wasm panic 后不再复用旧核心，而是在新 VM 上继续完整矩阵；
  避免未展开的线性内存栈影响后续用例。详见 [系统读取续执行](ir-system-read-continuation.md)。
- CFG 差分 reset 显式启用 SSE，且要求向量用例产生实际退休；原脚本可能因 debug
  OSFXSR 观察退出而没有真正执行 SIMD。脚本现在也实际接受 debug/release 核心参数。
  修复后最终两种核心均通过 75,264 正常 CFG、37,632 精确预算、32 第二轮缺页及 16 跳过故障臂对照。

### 未合入默认策略的探索

以下单轮样本仅用于否定调参方向，不是性能验收。所有实验保持 XP 镜像、执行预算、
CPUID/TSC 和退休计数语义不变：

| 核心/设置 | 启动显示里程碑秒数 | mIPS | 判断 |
|---|---:|---:|---|
| stage2 默认 | 27.670 | 62.929 | 参照 |
| stage2 cache 768 | 26.903 | 59.981 | 没有同时改善两项指标 |
| stage2 heat 512 | 32.229 | 50.266 | 编译/淘汰压力增加 |
| stage3 默认 | 26.682 | 61.085 | 仍明显慢于 legacy |
| stage3 阈值 32/128 | 28.405 | 61.363 | 更多编译，启动变慢 |
| stage3 阈值 16/64 | 30.637 | 59.042 | 启动与吞吐均变差 |

保留默认 cache=256、heat=128、阈值=64/256。stage3 无诊断 V8 采样将约 19.8%
归为解释器、19.0% 为生成代码及其 helper、16.9% 为 IR admission/dispatch、9.5%
为 IR 编译、17.6% 为 idle。采样会扰动运行且分类受内联影响，不能替代关闭采样的配对结果。
热度表的约 1,415 万次替换未区分解释入口与已发布 Tier 1；不能据此直接认定晋级
丢失占比。stage5 已实现独立的 cache 每别名热度、完整失败源码抑制、配置重置与 owner 生命周期，但 XP 探索样本未证明收益，默认关闭。详见 [缓存内晋升热度实验](ir-resident-promotion.md)。


### Stage4 正确性验证

- `RUSTFLAGS='-D warnings' cargo test`：284 passed、0 failed、4 ignored。
- Wasm 全套实际执行通过：宽索引、ABI、SSA/copy cycles、恢复状态、动态计数、
  owned MIR、SIMD/scalar/SCCP/LICM 及 fresh locals；不可变 XP replay 2,540/2,540。
- 最终 debug/release 每核 CFG 75,264 对照、37,632 精确预算、32 第二轮缺页、16
  未执行故障臂；entry 2,400 拒绝、240 正常、64 缺页；共享入口 48 对照及 96 拒绝。
- 两种核心的 CpuReload 182、STI 1,026、IR10 12 standalone + 8 CPU、VERR/VERW
  raw/computed ZF 故障对照通过。register corpus 对解释器及 legacy 各 1,890 例通过。
- CR/DR/CPUID 原矩阵及新续执行/日志/直接 ABI 矩阵通过，数量见系统读取说明。
- runtime/cache debug/release 的自动编译、发布、SMC、映射、reset/restore、热源调度、
  已观察侧入口、哈希碰撞、warm chain、diagnostics、I/O observer 回归通过。
  warm chain 每核 13 配对场景、9,133 次交接；fallback 自动编译 609 次发布。
- 无 SIMD 的 test/runtime 核心构建、fallback 与 auto 回归通过。
- 真实 Chromium main/Worker IR 配置与恢复、Worker 设备/图形检查点、AudioWorklet
  三套通过，确认 IR 模式没有 legacy JIT generation。
- decoder 16,384 NDISASM 边界对照、生成目录检查及 experimental completeness
  通过。仍为 935 编码、3,972 粗粒度形式、0 experimental Pending、3,728 production Pending。

主要日志：`build/ir-review-current-native-edge-locals.log`、
`build/ir-review-current-wasm-edge-locals.log`、`build/ir-next-stage4-coverage.log`、
`build/ir-next-stage4-lifecycle-*.log`、`build/ir-next-stage4-portable.log`、
`build/ir-next-stage4-browser.log`。


### Stage4 XP 三轮配对（未通过）

| 后端 | 启动里程碑中位秒数 | 中位 mIPS |
|---|---:|---:|
| 原 IR | 27.274 | 59.580 |
| stage4 IR | 28.557 | 63.758 |
| legacy（同核心） | 20.002 | 127.375 |

吞吐比原 IR 提高约 7.0%，但启动时间增加约 4.7%，仍只有 legacy 的约一半
吞吐。本组不通过验收，不能声称已完成性能目标。详见
[三轮记录](performance/ir-runtime-followup-20260923/stage4-xp-summary.json)。

后续单次把 heat=512 与 cache=768 同时扩大，得到 33.161 秒 / 51.815 mIPS，
2,053 次 Tier1 发布、639 次 Tier2 发布、1,093 次淘汰；IR 覆盖率约 83.5%。
更大的热度集合引入了过多编译，不能仅靠扩大容量消除启动瓶颈。2048 cache
请求被原有 256～768 的配置边界正常拒绝，没有更改容量上限。
