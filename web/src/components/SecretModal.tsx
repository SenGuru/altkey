import { useState } from 'react';

interface SecretModalProps {
  title: string;
  secret: string;
  instructions: string;
  onClose: () => void;
}

/**
 * Displays a secret (ak_agent_ or ak_live_) exactly once.
 * The user must copy it and confirm before dismissing.
 */
export function SecretModal({ title, secret, instructions, onClose }: SecretModalProps) {
  const [copied, setCopied] = useState(false);

  function handleCopy() {
    navigator.clipboard.writeText(secret).then(
      () => setCopied(true),
      () => {
        // Fallback: select the text manually
      },
    );
  }

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="secret-modal-title">
      <div className="modal">
        <h2 className="modal-title" id="secret-modal-title">{title}</h2>

        <p className="modal-instructions">{instructions}</p>

        <div className="secret-box-wrapper">
          <code className="secret-box" aria-label="Secret value">{secret}</code>
        </div>

        <button
          className={`btn${copied ? ' btn-secondary' : ' btn-primary'}`}
          type="button"
          onClick={handleCopy}
        >
          {copied ? 'Copied!' : 'Copy'}
        </button>

        <p className="modal-warning" role="alert">
          ⚠ This is shown only once — save it now. It cannot be retrieved after you close this dialog.
        </p>

        <div className="modal-actions">
          <button
            className="btn btn-primary"
            type="button"
            onClick={onClose}
          >
            I&apos;ve saved it
          </button>
        </div>
      </div>
    </div>
  );
}
