# OPEN_QUESTIONS.md

This file tracks open questions about the `ai-bridge` project that need
clarification before proceeding to the next phase.

## Current questions

1. **تطبيق الطرفية الرسمي (Official Terminal App)**: بخصوص `org.gnome.Terminal` في قائمة السماح (`config/allowlist.toml`)، هل هناك تطبيق طرفية (Terminal) آخر تفضل اعتماده رسمياً في المشروع؟ هذا يؤثر على `hand-eye` portal authorization و executor allowlist.

2. **مسار قاعدة بيانات تطبيقات الدردشة (Chat App DB Path)**: حول وحدة `ai-bridge-desktop-io`: ما هو المسار وقاعدة البيانات (SQLite/Cache/JSON) المحددة التي تعتمدها تطبيقات الدردشة (المايسترو، A، B) لحفظ سجلات الردود، وذلك لجدولتها بدقة في المزامنة؟ حالياً `default_store_root()` يستخدم fallback إلى `~/.local/share/ai-bridge-chat` أو `/tmp/ai-bridge-chat`.

3. **إدخال نص حقيقي عبر RemoteDesktop / PipeWire**: ما هو مسار الإدخال النصي الموثوق المطلوب في الواجهة الحقيقية؟ هل يدعم `ashpd`/`RemoteDesktop` إدخال Unicode نصي مباشر، أم يلزم استخدام مكتبة لوحة مفاتيح حقيقية مع خريطة keycodes؟ هذا يُعدّ شرطاً أساسياً قبل تنفيذ حقن النص الفعلي في محيط التطبيق الجديد.

4. **قراءة إطار فعلي من ScreenCast**: كيف تُقرأ بايتات الإطار الفعلية من تدفق PipeWire/stream الذي يفتح `screen_cast_capture()`؟ لا يوجد اليوم في المشروع أي مسار يقرأ `frame data` فعلياً من الجلسة، وهو شرط أساسي لتوفير صورة حقيقية إلى الحافظة.

## الأسئلة المحلولة (مرجع)

- ~~فئتا Gatekeeper: تأكيد أن هناك فئتين فقط (Delegable/NonDelegable) — تم التنفيذ والتحقق~~
- ~~SecureToken distribution: Case B (ملف .token) لـ public_maestro — تم التنفيذ والتحقق~~
- ~~Executor allowlist: قائمة سماح مستقلة للثنائيات — تم التنفيذ والتحقق~~
- ~~Sub-chat limit: `sub_chat_limit() = 3` (تم التعديل من 2 إلى 3، الاختبارات محدثة)~~
- **UI manual plan route**: تم اختيار إرسال خطة التشغيل اليدوية عبر نفس قناة `public_maestro.sock` بصيغة `ExecutionPlan` العادية، لا عبر مسار خاص أو bypass؛ هذا يضمن أن المسار اليدوي هو نفسه المسار الخارجي تماماً، ومنعاً لأي shortcut غير موثق.
- **Backward-compatible tag policy**: عند عدم وجود وسم `[[AB:...]]` صريح، يبقى تقييم الأوامر مطابق للفحص النصي القديم؛ أما عند وجود وسم واضح، فيُطبّق جدول الرموز المشترك ويُعدّ التصنيف الرمزي هو القيد الأقوى. هذا القرار تمّ توثيقه صراحةً في `criteria.rs` لتجنب افتراضات صامتة.
