"""Generate a local CA + a leaf cert covering the AI provider API domains.

For PERSONAL use on your own machine only. The CA private key stays in this
folder — guard it; anyone with it can MITM your HTTPS. Run teardown.ps1 to
remove the CA from your trust store when done.
"""
import datetime
import ipaddress
from pathlib import Path

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID

HERE = Path(__file__).parent

# Domains altkey can transparently serve (OpenAI-wire + Anthropic-wire only).
DOMAINS = [
    "api.openai.com",
    "chatgpt.com",
    "chat.openai.com",
    "api.anthropic.com",
]


def _save(path: Path, data: bytes):
    path.write_bytes(data)
    print(f"  wrote {path.name}")


def main():
    now = datetime.datetime.utcnow()

    # --- Root CA ---
    ca_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    ca_name = x509.Name([
        x509.NameAttribute(NameOID.COMMON_NAME, "altkey local CA"),
        x509.NameAttribute(NameOID.ORGANIZATION_NAME, "altkey (personal)"),
    ])
    ca_cert = (
        x509.CertificateBuilder()
        .subject_name(ca_name)
        .issuer_name(ca_name)
        .public_key(ca_key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(days=1))
        .not_valid_after(now + datetime.timedelta(days=3650))
        .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
        .add_extension(x509.KeyUsage(digital_signature=True, key_cert_sign=True, crl_sign=True,
                                     key_encipherment=False, content_commitment=False,
                                     data_encipherment=False, key_agreement=False,
                                     encipher_only=False, decipher_only=False), critical=True)
        .sign(ca_key, hashes.SHA256())
    )

    # --- Leaf cert (one cert, SANs for every intercepted domain) ---
    leaf_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    san = [x509.DNSName(d) for d in DOMAINS]
    leaf_cert = (
        x509.CertificateBuilder()
        .subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, DOMAINS[0])]))
        .issuer_name(ca_name)
        .public_key(leaf_key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(days=1))
        .not_valid_after(now + datetime.timedelta(days=825))
        .add_extension(x509.SubjectAlternativeName(san), critical=False)
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
        .add_extension(x509.ExtendedKeyUsage([x509.oid.ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
        .sign(ca_key, hashes.SHA256())
    )

    _save(HERE / "ca.crt", ca_cert.public_bytes(serialization.Encoding.PEM))
    _save(HERE / "ca.key", ca_key.private_bytes(serialization.Encoding.PEM,
          serialization.PrivateFormat.TraditionalOpenSSL, serialization.NoEncryption()))
    _save(HERE / "server.crt", leaf_cert.public_bytes(serialization.Encoding.PEM))
    _save(HERE / "server.key", leaf_key.private_bytes(serialization.Encoding.PEM,
          serialization.PrivateFormat.TraditionalOpenSSL, serialization.NoEncryption()))
    print("\nDone. Intercepted domains:")
    for d in DOMAINS:
        print(f"  {d}")
    print("\nNext: run setup.ps1 (as Administrator) to install the CA + hosts entries.")


if __name__ == "__main__":
    main()
