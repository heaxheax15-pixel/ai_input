# OPEN_QUESTIONS.md

This file tracks open questions about the `ai-bridge` project that need
clarification before proceeding to the next phase.

## Current questions

1. **تطبيق الطرفية الرسمي (Official Terminal App)**: بخصوص `org.gnome.Terminal` في قائمة السماح (`config/allowlist.toml`)، هل هناك تطبيق طرفية (Terminal) آخر تفضل اعتماده رسمياً في المشروع؟ هذا يؤثر على `hand-eye` portal authorization و executor allowlist.

2. **مسار قاعدة بيانات تطبيقات الدردشة (Chat App DB Path)**: حول وحدة `ai-bridge-desktop-io`: ما هو المسار وقاعدة البيانات (SQLite/Cache/JSON) المحددة التي تعتمدها تطبيقات الدردشة (المايسترو، A، B) لحفظ سجلات الردود، وذلك لجدولتها بدقة في المزامنة؟ حالياً `default_store_root()` يستخدم fallback إلى `~/.local/share/ai-bridge-chat` أو `/tmp/ai-bridge-chat`.

3. **الحد الأقصى للمحادثات الفرعية (Sub-chat limit)**: التصميم الحالي يحدد `sub_chat_limit() = 2` في `Branch` (أي 3 استدعاءات إجمالاً بما في ذلك الاستدعاء الأول). هل هذا الرقم نهائي أم قابل للتعديل عبر تهيئة؟

## الأسئلة المحلولة (مرجع)

- ~~فئتا Gatekeeper: تأكيد أن هناك فئتين فقط (Delegable/NonDelegable) — تم التنفيذ والتحقق~~
- ~~SecureToken distribution: Case B (ملف .token) لـ public_maestro — تم التنفيذ والتحقق~~
- ~~Executor allowlist: قائمة سماح مستقلة للثنائيات — تم التنفيذ والتحقق~~
