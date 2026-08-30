# ai-bridge — سجل التقدم (PROGRESS.md)

هذا الملف هو سجل الاستمرارية للجلسة. كل وحدة عمل مكتملة تُضاف هنا بملخص ما أُنجز، القرارات التقنية المتخذة ذاتياً، ونتيجة `cargo test/clippy`.

---

## الوحدة 1: تصالح حالة المستودع — حل تعارض المقابس (القسم 1.3، 1.5)

**تاريخ الإنجاز**: 2025-08-30

### ما أُنجز
1. **فحص `PermanentSuspension`/`PermanentlySuspended`/`is_system_wipe_hint`**: `grep` شامل على كل ملفات `.rs` — **لا وجود لأي أثر** لهذه الرموز. المبدأ المعماري (فئتان فقط: Delegable/NonDelegable) مطبق بالفعل.
2. **توثيق انقسام gatekeeper**: الحزمتان موجودتان بالفعل:
   - `ai-bridge-gatekeeper-core` — منطق القرار النقي: `criteria.rs`، `policy.rs`، `timers.rs` (يحتوي `evaluate_timeout` + `await_decision`)، الاختبارات `criteria_tests.rs`. **صفر تبعيات workspace** غير `ai-bridge-protocol`.
   - `ai-bridge-gatekeeper-daemon` — الثنائيات + IPC + التنفيذ: `gatekeeper.rs` (منطق ActiveTasks/serve_ui_session)، `executor.rs`، `mock_gatekeeper.rs`، `ipc_e2e_test.rs`. يعتمد على `ai-bridge-gatekeeper-core` + `ai-bridge-channels` + `ai-bridge-protocol`.
3. **حل تعارض `public_maestro.sock`**: وُجد ثنائان يحاولان `bind` على نفس المقبس:
   - الديمون الرئيسي: `src/main.rs` → يربط القنوات الثلاث + ينفذ `event_loop.rs` كاملاً.
   - الثنائي المستقل: `crates/ai-bridge-gatekeeper-daemon/src/bin/gatekeeper.rs` — يربط `public_maestro.sock` ويكرر نفس منطق التصنيف/التنفيذ.
   **القرار المتخذ ذاتياً**: حذف `gatekeeper.rs` بالكامل. دوره مكرر 100% مع الديمون الرئيسي. إبقاء `mock_gatekeeper.rs` للاختبارات اليدوية/التلقائية (`ipc_e2e_test.rs`).
4. **تحديث `Cargo.toml` للجذر**: كان يحتوي بالفعل مراجع للحزمتين الجديدتين (`gatekeeper-core` و `gatekeeper-daemon`) — لا تعديل مطلوب.
5. **إزالة تعريف `[[bin]] name="gatekeeper"`** من `crates/ai-bridge-gatekeeper-daemon/Cargo.toml`.

### نتيجة التحقق
```
cargo test --workspace
→ 71 اختبار نجح، 0 فشل
cargo clippy --workspace --all-targets -- -D warnings
→ يمر بلا أخطاء/تحذيرات
```

---

## القرارات التقنية المتخذة ذاتياً (غير منصوص عليها حرفياً)

| القرار | المبرر |
|----------|---------|
| حذف `gatekeeper.rs` الثنائي المستقل | تعارض bind على `public_maestro.sock` + تكرار كامل للمنطق مع الديمون الرئيسي |
| إبقاء `mock_gatekeeper.rs` | مطلوب لاختبار IPC E2E، لا يربط في الإنتاج |
| لا آلية توكن لـ `private_a`/`private_b` حالياً | لا متصل خارجي اليوم؛ Branch/OrphanWorker استدعاءات داخل العملية فقط |

---

## النقاط المعلقة (ستعالج في الوحدات اللاحقة)

- [ ] **الوحدة 2**: جعل فشل `set_permissions` في `write_token_file` قاتلاً (القسم 4.3) — حالياً `let _ = ...` يهمل الفشل بصمت.
- [ ] **الوحدة 3**: توثيق القنوات الداخلية المعلقة في PROGRESS.md (القسم 4.4).
- [ ] **الوحدة 4**: قائمة سماح الثنائيات القابلة للتنفيذ (القسم 5 كاملاً).
- [ ] **الوحدة 5**: تنظيف نهائي + تحقق Git + تحديث OPEN_QUESTIONS.md (القسم 6).