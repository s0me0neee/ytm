import type { ReactNode } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";

interface ModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  icon?: ReactNode;
  width?: string;
  children: ReactNode;
}

/** A centred panel over a dimmed window; Esc, a click outside and the close button all dismiss it. */
export function Modal({ open, onClose, title, icon, width = "max-w-md", children }: ModalProps) {
  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="modal-overlay fixed inset-0 z-50 bg-black/55 backdrop-blur-sm" />
        <Dialog.Content
          className={`modal-content fixed top-1/2 left-1/2 z-50 flex max-h-[80vh] w-[calc(100vw-2rem)] ${width} -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-2xl border-[0.5px] border-hairline bg-raised shadow-2xl shadow-black/60 focus:outline-none`}
          aria-describedby={undefined}
        >
          <div className="flex flex-shrink-0 items-center justify-between px-5 pt-4 pb-3">
            <Dialog.Title className="flex items-center gap-2 text-heading font-semibold text-ink">
              {icon}
              {title}
            </Dialog.Title>
            <Dialog.Close className="rounded-full p-1 text-ink-faint hover:bg-surface-2 hover:text-ink" aria-label="Close">
              <X size={16} />
            </Dialog.Close>
          </div>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
