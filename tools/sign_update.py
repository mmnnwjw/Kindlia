"""Sign a Kindlia ZIP locally; the private key is never included in a package."""
import argparse,hashlib,os
from pathlib import Path
from nacl.signing import SigningKey
PUBLIC_KEY = "5add9eb805ce8d3faaffaeb1b2d027dd771e2f86178b1a39e4ea8bbafa442f2a"
def main():
    p=argparse.ArgumentParser();p.add_argument('package',type=Path);p.add_argument('--key',type=Path,default=Path.home()/'.kinovel-secrets/KINDLIA_UPDATE_SIGNING_KEY');a=p.parse_args()
    key=SigningKey(bytes.fromhex(a.key.read_text(encoding='ascii').strip()))
    if key.verify_key.encode().hex()!=PUBLIC_KEY:p.error('Signing key does not match Kindlia public key')
    sha=hashlib.sha256(a.package.read_bytes()).hexdigest();message=f'kindlia-update-v1\n{a.package.name}\n{sha}\n'.encode();sig=key.sign(message).signature
    key.verify_key.verify(message,sig)
    a.package.with_name(a.package.name+'.sig').write_text(sig.hex()+'\n',encoding='ascii')
    print(f'Signed {a.package.name}; SHA256 {sha}')

if __name__=="__main__":
    main()
