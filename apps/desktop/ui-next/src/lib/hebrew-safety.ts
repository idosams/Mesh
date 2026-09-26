/** Display-only translations of exact source-owned confirmation templates. Captured paths are isolated, never normalized. */
const isolate = (value: string) => `\u2068${value}\u2069`;
export function translateSafety(text: string): string | null {
  let match: RegExpMatchArray | null;
  if ((match = text.match(/^Delete (.+) from (.+)\? Saved file content remains in immutable history, but the current working (file|folder) will be removed\.$/s)))
    return `למחוק את ${isolate(match[1])} מתוך ${isolate(match[2])}? תוכן קבצים שנשמר נשאר בהיסטוריה שאינה ניתנת לשינוי, אך ${match[3] === 'file' ? 'קובץ העבודה הנוכחי יימחק' : 'תיקיית העבודה הנוכחית תימחק'}.`;
  if ((match = text.match(/^Delete (.+)\?$/s))) return `למחוק את ${isolate(match[1])}?`;
  if ((match = text.match(/^(Create|Replace) (.+) in (.+) from saved Mesh version (.+)\? Mesh history and the managed working file will not change\.$/s)))
    return `${match[1] === 'Create' ? 'ליצור' : 'להחליף'} את ${isolate(match[2])} בתוך ${isolate(match[3])} מתוך גרסת Mesh השמורה ${isolate(match[4])}? היסטוריית Mesh וקובץ העבודה המנוהל לא ישתנו.`;
  if ((match = text.match(/^Create (\d+) saved folders? in (.+)\? Mesh creates only absent folders in depth order, rechecking every exact parent, then previews files separately\.$/s)))
    return `ליצור ${match[1]} תיקיות שמורות בתוך ${isolate(match[2])}? Mesh יוצר רק תיקיות חסרות לפי סדר העומק, בודק מחדש כל תיקיית אב ומציג את הקבצים בנפרד בתצוגה מקדימה.`;
  if ((match = text.match(/^Remove (\d+) unchanged old files? from (.+)\? Every file is moved aside, reverified against its last saved bytes and metadata, then removed\. Changed and unrelated files are preserved\.$/s)))
    return `להסיר ${match[1]} קבצים ישנים שלא השתנו מתוך ${isolate(match[2])}? כל קובץ מועבר הצידה, מאומת מחדש מול התוכן והנתונים האחרונים שנשמרו ורק אז מוסר. קבצים שהשתנו או שאינם קשורים נשמרים.`;
  if ((match = text.match(/^Remove (\d+) old folders? from (.+)\? Mesh attempts exact identities deepest first and can remove only empty folders\. Recursive deletion is unavailable\.$/s)))
    return `להסיר ${match[1]} תיקיות ישנות מתוך ${isolate(match[2])}? Mesh בודק זהויות מדויקות מהתיקיות העמוקות כלפי מעלה ומסיר רק תיקיות ריקות. מחיקה רקורסיבית אינה זמינה.`;
  if ((match = text.match(/^Update (\d+) proven files? in (.+)\? Mesh keeps (\d+) unproven files? unchanged\. Each selected file is installed atomically; if a later file changes, Mesh stops and keeps the already-completed prefix\.$/s)))
    return `לעדכן ${match[1]} קבצים מאומתים בתוך ${isolate(match[2])}? Mesh משאיר ${match[3]} קבצים שלא אומתו ללא שינוי. כל קובץ נבחר מותקן באופן אטומי; אם קובץ מאוחר יותר השתנה, Mesh עוצר ומשאיר את העדכונים שכבר הושלמו.`;
  if ((match = text.match(/^Roll back the unchanged managed copy at (.+)\? The original folder remains untouched, but this private working copy and its local Mesh history will be removed\.$/s)))
    return `לבטל את העותק המנוהל שלא השתנה במיקום ${isolate(match[1])}? התיקייה המקורית נשארת ללא שינוי, אך עותק העבודה הפרטי והיסטוריית Mesh המקומית שלו יימחקו.`;
  if ((match = text.match(/^(Create|Remove|Update) (\d+) (saved folders?|unchanged old files?|old folders?|proven files?)\?$/)))
    return `${match[1] === 'Create' ? 'ליצור' : match[1] === 'Remove' ? 'להסיר' : 'לעדכן'} ${match[2]} ${match[3].includes('folder') ? 'תיקיות' : 'קבצים'}?`;
  if ((match = text.match(/^(Create|Remove|Update) (\d+) (saved folders?|unchanged old files?|old empty folders?|changed files?)$/)))
    return `${match[1] === 'Create' ? 'יצירת' : match[1] === 'Remove' ? 'הסרת' : 'עדכון'} ${match[2]} ${match[3].includes('folder') ? 'תיקיות' : 'קבצים'}`;
  return null;
}
export function hebrewDiagnostic(message: string): string {
  if (message.includes('workspace-version-empty')) return `בגרסה השמורה אין קבצים או תיקיות. עדיין אי אפשר לפתוח גרסה ריקה כתיקיית עבודה חדשה. בחרו גרסה שמכילה קבצים או תיקיות. סביבת העבודה הנוכחית לא השתנתה ולא נוצרה תיקייה.\n\n${message}`;
  if (message.includes('folder-import-empty') || (message.includes('folder-import-refused') && /empty|no ordinary|no project|no importable/i.test(message))) return `התיקייה אינה מכילה קבצים או תיקיות רגילים שניתן לייבא. בחרו תיקייה עם קובץ פרויקט או תת־תיקייה; נתוני Git לבדם אינם מספיקים.\n\n${message}`;
  return `הפעולה דיווחה על בעיה. בדקו את המצב לפני ניסיון נוסף; ייתכן שחלק מהפעולות כבר הושלמו. פרטי האבחון המקוריים מופיעים למטה.\n\n${message}`;
}
