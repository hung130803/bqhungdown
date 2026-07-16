/**
 * Màn hình trống thân thiện dùng chung — icon lớn trong vòng tròn + tiêu đề +
 * gợi ý ngắn. Thay cho dòng chữ trơn, giúp người mới hiểu cần làm gì.
 * Thuần trình bày (không state/logic) nên an toàn tuyệt đối.
 */
interface Props {
  icon?: string;
  title: string;
  hint?: string;
  /** Nút hành động tuỳ chọn (vd "Dán link", "Tải mới"). */
  action?: React.ReactNode;
}

export function EmptyState({ icon = "📥", title, hint, action }: Props) {
  return (
    <div className="flex flex-col items-center justify-center text-center py-16 px-6 bqd-page">
      <div className="w-16 h-16 rounded-full bg-surface-2 border border-border flex items-center justify-center text-3xl mb-4 select-none">
        {icon}
      </div>
      <p className="text-fg font-medium">{title}</p>
      {hint && <p className="text-muted text-sm mt-1.5 max-w-xs">{hint}</p>}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}
