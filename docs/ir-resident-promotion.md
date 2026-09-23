# 缓存内 Tier 1 晋升热度实验

`ir_cache_set_resident_promotion(1)` 是启动时的可选策略，默认关闭。设置时必须处于冷点、自动调度关闭、调度器为空且 IR 缓存没有任何 artifact。策略像缓存容量一样保留到 reset/restore 后；`ir_auto_config` 每次成功调用都会重置当前 owner 的晋升热度、失败指纹和扫描游标。

原有 128 个 PC 的热度环同时服务尚未编译的 PC、已发布 Tier 1 及可继续融合的 Tier 2。已发布 Tier 1 在每次 admission 时重新插入环；大于 128 个 PC 的循环可以不断替换热度而始终无法晋升。该实验把 Tier 1 的计数移到已发布 owner 的每个可调用 alias，使用独立、饱和的 `u32` 计数。计数发生在 admission 成功后、生成代码调用前，与原有 `note_cached` 时点一致，自动调度关闭时不增加。共享模块的 alias 不合并热度。原有 linked visit 统计仍然保留；Tier 2 融合继续使用原热度路径。

热度达到 promote 阈值后保持 ready，不使用可能溢出或丢失候选的一次性队列。每帧至多消费一次扫描信用，缓存扫描最多检查 128 个 owner/alias 位置，原热度环扫描最多检查 128 个位置；索引集合本身受缓存容量上限约束。可晋升的完整 ticket 为 `(CpuEntryKey, 非回绕 job ID)`，扫描游标只是位置提示。两种扫描域轮流优先，已有可融合 Tier 2 不会被持续 ready 的 Tier 1 owner 完全压住。刚达到阈值的解释执行 Tier 1 编译继续优先；这种持续的优先流量仍可能推迟后台晋升，保持原调度策略。若本帧已经扫描过，后来达到阈值的缓存 entry 要等下一帧；hot path 不新增即时编译提示。

晋升不授予代码执行或发布权限。原有 CPU 上下文、源字节、映射、generation、预算、SMC 和 retirement 检查不变。异步安装在 validate 和 finish 两个阶段额外检查父 owner 的完整 ticket，旧的晋升 Promise 不能覆盖期间手动发布的新 owner。单独替换一个共享 alias 只删除该 alias 的热度，保留其他 alias；SMC、失效、淘汰和 reset 使旧 owner 的提示失效。

失败指纹保存在该 alias 的 owner 内，包含整个 Tier 2 捕获范围的字节和映射，包括编译器最终没有纳入 artifact 的尾部。后续选中同一候选仍然消费编译信用并重新捕获源，但相同指纹不会再次编译；尾部原始字节或映射变化后可以重试。指纹也区分“尚未失败”和“捕获失败返回 None”。编译成功后异步安装失败保留同一完整指纹；新的 owner、配置重置不会继承旧失败状态。

诊断接口：cache stat 41 为开关，42 为已扫描位置，43 为选择次数，44 为 admission 热度更新次数，45 为相同失败源抑制次数。entry stat 15 为完整 alias 的热度，16 为失败指纹存在，17 为达到当前启用阈值，18/19 为 owner ID 低/高 32 位。这些字段仅观察调度状态，不提供 admission 或发布权限。

实验有额外的 owner 内存、冷点扫描和可能增多的 Tier 2 编译成本。默认策略是否应切换必须由相同快照和相同配置的 XP 成对测试决定；保住热度不等于已经证明总吞吐提升。

初步验证：三个原生单元测试覆盖饱和、独立 alias、空/非空失败捕获、字节/映射差异和跨 128 位置扫描。实际 runtime 与 cache-test-release 上的 `resident_promotion.mjs` 都通过四组对抗场景，包括 160 个已发布 owner 的确定性分帧 A/B、raw FLAGS/寄存器/退休计数一致性、异步旧 owner 拒绝和完整尾部失败指纹重试。既有 cache、auto、warm_chain、scheduler_ready、observed_entries、missing_hint 六项回归通过。具体日志在 `build/ir-next-stage5-{resident-coverage,lifecycle}-*.log`；这些正确性结果尚不代表 XP 性能收益。
