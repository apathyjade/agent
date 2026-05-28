import { useEffect, useState } from 'react';
import { X, CheckCircle, AlertCircle, AlertTriangle, Info } from 'lucide-react';
import { Row, Col } from '@jelper/component';
import { useStore, type Toast as ToastType } from '../../store';

const iconMap = {
  success: CheckCircle,
  error: AlertCircle,
  warning: AlertTriangle,
  info: Info,
};

const colorMap = {
  success: 'bg-green-50 border-green-200 text-green-800 dark:bg-green-900/30 dark:border-green-800 dark:text-green-300',
  error: 'bg-red-50 border-red-200 text-red-800 dark:bg-red-900/30 dark:border-red-800 dark:text-red-300',
  warning: 'bg-amber-50 border-amber-200 text-amber-800 dark:bg-amber-900/30 dark:border-amber-800 dark:text-amber-300',
  info: 'bg-blue-50 border-blue-200 text-blue-800 dark:bg-blue-900/30 dark:border-blue-800 dark:text-blue-300',
};

function ToastItem({ toast }: { toast: ToastType }) {
  const removeToast = useStore((s) => s.removeToast);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    requestAnimationFrame(() => setVisible(true));
  }, []);

  const Icon = iconMap[toast.type];

  return (
    <Row
      $align="flex-start"
      $gap={12}
      className={`px-4 py-3 rounded-lg border shadow-lg transition-all duration-300 ${
        colorMap[toast.type]
      } ${visible ? 'opacity-100 translate-y-0' : 'opacity-0 translate-y-2'}`}
    >
      <Row.Item $fixed><Icon size={18} className="mt-0.5" /></Row.Item>
      <Row.Item $scale={1}><span className="text-sm">{toast.message}</span></Row.Item>
      <Row.Item $fixed>
        <button onClick={() => removeToast(toast.id)} className="p-0.5 rounded hover:opacity-70 transition-opacity">
          <X size={14} />
        </button>
      </Row.Item>
    </Row>
  );
}

export function ToastContainer() {
  const toasts = useStore((s) => s.toasts);

  if (toasts.length === 0) return null;

  return (
    <Col $gap={8} className="fixed top-4 right-4 z-50 max-w-sm w-full pointer-events-none">
      {toasts.map((toast) => (
        <div key={toast.id} className="pointer-events-auto">
          <ToastItem toast={toast} />
        </div>
      ))}
    </Col>
  );
}
