import { useEffect } from "react";

export interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: React.ReactNode;
  confirmText?: string;
  cancelText?: string;
  variant?: "danger" | "primary";
  /** Optional secondary checkbox (e.g. "also delete files"). */
  extraToggle?: { label: string; value: boolean; onChange: (v: boolean) => void };
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  open,
  title,
  message,
  confirmText = "Đồng ý",
  cancelText = "Huỷ",
  variant = "primary",
  extraToggle,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
      if (e.key === "Enter") onConfirm();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [open, onCancel, onConfirm]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 bg-black/60 z-50 flex items-center justify-center p-4" onClick={onCancel}>
      <div
        className="bg-bg border border-border rounded-lg p-5 w-full max-w-md space-y-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="text-lg font-medium text-fg">{title}</h3>
        <div className="text-sm text-muted">{message}</div>
        {extraToggle && (
          <label className="flex items-center gap-2 text-sm text-fg cursor-pointer">
            <input
              type="checkbox"
              checked={extraToggle.value}
              onChange={(e) => extraToggle.onChange(e.target.checked)}
              className="h-4 w-4"
            />
            <span>{extraToggle.label}</span>
          </label>
        )}
        <div className="flex justify-end gap-2 pt-2">
          <button
            onClick={onCancel}
            className="px-3 py-2 rounded-md bg-surface-2 border border-border text-fg text-sm hover:bg-surface"
          >
            {cancelText}
          </button>
          <button
            onClick={onConfirm}
            className={`px-3 py-2 rounded-md text-sm text-white ${
              variant === "danger" ? "bg-danger hover:opacity-90" : "bg-accent hover:opacity-90"
            }`}
          >
            {confirmText}
          </button>
        </div>
      </div>
    </div>
  );
}
