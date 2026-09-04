using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Globalization;
using System.Linq;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Security.Cryptography.Pkcs;
using System.Security.Cryptography.X509Certificates;

namespace DocumentStudio.G04DC.Provenance {
    // Native Windows Cryptographic APIs provide the independent online and offline trust primitives.
    public sealed class FileTrustEvidence {
        public int status { get; set; }
        public string statusHex { get; set; }
        public bool passed { get; set; }
        public bool hashOnly { get; set; }
        public bool cacheOnlyUrlRetrieval { get; set; }
        public int signerCount { get; set; }
        public int timestampSignerCount { get; set; }
        public string signerLeafDerSha256 { get; set; }
        public string timestampLeafDerSha256 { get; set; }
        public string timestampUtc { get; set; }
        public int providerSignerError { get; set; }
        public int providerTimestampError { get; set; }
        public string failureOperation { get; set; }
    }

    public sealed class CertificateEvidence {
        public int chainPosition { get; set; }
        public string derSha256 { get; set; }
        public string thumbprint { get; set; }
        public string serialNumber { get; set; }
        public string subjectNameSha256 { get; set; }
        public string issuerNameSha256 { get; set; }
        public string publicKeyAlgorithmOid { get; set; }
        public int publicKeySizeBits { get; set; }
        public string signatureAlgorithmOid { get; set; }
        public string[] ekuOids { get; set; }
        public string notBeforeUtc { get; set; }
        public string notAfterUtc { get; set; }
        public bool selfSignedName { get; set; }
        public byte[] der { get; set; }
    }

    public sealed class ChainEvidence {
        public string purpose { get; set; }
        public bool onlineRevocation { get; set; }
        public bool exclusiveRoot { get; set; }
        public bool urlRetrievalDisabled { get; set; }
        public int errorStatus { get; set; }
        public string errorStatusHex { get; set; }
        public string[] errorStatusNames { get; set; }
        public int infoStatus { get; set; }
        public int policyError { get; set; }
        public string policyErrorHex { get; set; }
        public int policyChainIndex { get; set; }
        public int policyElementIndex { get; set; }
        public bool complete { get; set; }
        public bool trustedRoot { get; set; }
        public bool certificateSignaturesValid { get; set; }
        public bool purposeEkuValid { get; set; }
        public bool revocationKnownGood { get; set; }
        public bool valid { get; set; }
        public string verificationTimeUtc { get; set; }
        public CertificateEvidence[] certificates { get; set; }
    }

    public sealed class ProxyConfigurationEvidence {
        public bool systemNoProxy { get; set; }
        public bool userAutoDetect { get; set; }
        public bool userAutoConfigUrlPresent { get; set; }
        public bool userNamedProxyPresent { get; set; }
        public bool proxyFree { get; set; }
    }

    public static class AuthenticodeProvenanceVerifier {
        private const uint X509_ASN_ENCODING = 0x00000001;
        private const uint CERT_STORE_CREATE_NEW_FLAG = 0x00002000;
        private const uint CERT_STORE_ADD_ALWAYS = 4;
        private const uint CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL = 0x00000004;
        private const uint CERT_CHAIN_DISABLE_AUTH_ROOT_AUTO_UPDATE = 0x00000100;
        private const uint CERT_CHAIN_TIMESTAMP_TIME = 0x00000200;
        private const uint CERT_CHAIN_DISABLE_AIA = 0x00002000;
        private const uint CERT_CHAIN_REVOCATION_ACCUMULATIVE_TIMEOUT = 0x08000000;
        private const uint CERT_CHAIN_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT = 0x40000000;
        private static readonly IntPtr CERT_CHAIN_POLICY_AUTHENTICODE = new IntPtr(2);
        private static readonly IntPtr CERT_CHAIN_POLICY_AUTHENTICODE_TS = new IntPtr(3);

        private const uint WTD_UI_NONE = 2;
        private const uint WTD_REVOKE_WHOLECHAIN = 1;
        private const uint WTD_CHOICE_FILE = 1;
        private const uint WTD_STATEACTION_VERIFY = 1;
        private const uint WTD_STATEACTION_CLOSE = 2;
        private const uint WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT = 0x00000080;
        private const uint WTD_DISABLE_MD2_MD4 = 0x00002000;
        private const uint PKCS_7_ASN_ENCODING = 0x00010000;
        private const uint SPC_INDIRECT_DATA_CONTENT_STRUCT = 2003;
        private const uint ERROR_INVALID_PARAMETER = 87;
        private const uint ERROR_INSUFFICIENT_BUFFER = 122;
        private const uint ERROR_MORE_DATA = 234;
        private const uint CRYPT_E_NO_MATCH = 0x80092009;
        private const uint WINHTTP_ACCESS_TYPE_NO_PROXY = 1;
        private const uint CRYPT_VERIFY_CERT_SIGN_SUBJECT_CERT = 2;
        private const uint CRYPT_VERIFY_CERT_SIGN_ISSUER_CERT = 2;
        private const uint CRYPT_VERIFY_CERT_SIGN_DISABLE_MD2_MD4_FLAG = 0x00000001;

        private const uint TRUST_IS_NOT_TIME_VALID = 0x00000001;
        private const uint TRUST_IS_REVOKED = 0x00000004;
        private const uint TRUST_IS_NOT_SIGNATURE_VALID = 0x00000008;
        private const uint TRUST_IS_NOT_VALID_FOR_USAGE = 0x00000010;
        private const uint TRUST_IS_UNTRUSTED_ROOT = 0x00000020;
        private const uint TRUST_REVOCATION_STATUS_UNKNOWN = 0x00000040;
        private const uint TRUST_IS_CYCLIC = 0x00000080;
        private const uint TRUST_INVALID_EXTENSION = 0x00000100;
        private const uint TRUST_INVALID_POLICY_CONSTRAINTS = 0x00000200;
        private const uint TRUST_INVALID_BASIC_CONSTRAINTS = 0x00000400;
        private const uint TRUST_INVALID_NAME_CONSTRAINTS = 0x00000800;
        private const uint TRUST_HAS_NOT_SUPPORTED_NAME_CONSTRAINT = 0x00001000;
        private const uint TRUST_HAS_NOT_DEFINED_NAME_CONSTRAINT = 0x00002000;
        private const uint TRUST_HAS_NOT_PERMITTED_NAME_CONSTRAINT = 0x00004000;
        private const uint TRUST_HAS_EXCLUDED_NAME_CONSTRAINT = 0x00008000;
        private const uint TRUST_IS_PARTIAL_CHAIN = 0x00010000;
        private const uint TRUST_CTL_IS_NOT_TIME_VALID = 0x00020000;
        private const uint TRUST_CTL_IS_NOT_SIGNATURE_VALID = 0x00040000;
        private const uint TRUST_CTL_IS_NOT_VALID_FOR_USAGE = 0x00080000;
        private const uint TRUST_HAS_WEAK_SIGNATURE = 0x00100000;
        private const uint TRUST_IS_OFFLINE_REVOCATION = 0x01000000;
        private const uint TRUST_NO_ISSUANCE_CHAIN_POLICY = 0x02000000;
        private const uint TRUST_IS_EXPLICIT_DISTRUST = 0x04000000;
        private const uint TRUST_HAS_NOT_SUPPORTED_CRITICAL_EXT = 0x08000000;

        private static readonly Guid GenericVerifyV2 = new Guid("00AAC56B-CD44-11D0-8CC2-00C04FC295EE");

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct WINTRUST_FILE_INFO {
            public uint cbStruct;
            public string pcwszFilePath;
            public IntPtr hFile;
            public IntPtr pgKnownSubject;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct WINTRUST_DATA {
            public uint cbStruct;
            public IntPtr pPolicyCallbackData;
            public IntPtr pSIPClientData;
            public uint dwUIChoice;
            public uint fdwRevocationChecks;
            public uint dwUnionChoice;
            public IntPtr pFile;
            public uint dwStateAction;
            public IntPtr hWVTStateData;
            public string pwszURLReference;
            public uint dwProvFlags;
            public uint dwUIContext;
            public IntPtr pSignatureSettings;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct FILETIME_NATIVE {
            public uint dwLowDateTime;
            public uint dwHighDateTime;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CRYPT_PROVIDER_SGNR {
            public uint cbStruct;
            public FILETIME_NATIVE sftVerifyAsOf;
            public uint csCertChain;
            public IntPtr pasCertChain;
            public uint dwSignerType;
            public IntPtr psSigner;
            public uint dwError;
            public uint csCounterSigners;
            public IntPtr pasCounterSigners;
            public IntPtr pChainContext;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CRYPT_PROVIDER_CERT {
            public uint cbStruct;
            public IntPtr pCert;
            public int fCommercial;
            public int fTrustedRoot;
            public int fSelfSigned;
            public int fTestCert;
            public uint dwRevokedReason;
            public uint dwConfidence;
            public uint dwError;
            public IntPtr pTrustListContext;
            public int fTrustListSignerCert;
            public IntPtr pCtlContext;
            public uint dwCtlError;
            public int fIsCyclic;
            public IntPtr pChainElement;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CRYPTOAPI_BLOB {
            public uint cbData;
            public IntPtr pbData;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CRYPT_ALGORITHM_IDENTIFIER {
            public IntPtr pszObjId;
            public CRYPTOAPI_BLOB Parameters;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct SIP_SUBJECTINFO {
            public uint cbSize;
            public IntPtr pgSubjectType;
            public IntPtr hFile;
            public string pwsFileName;
            public string pwsDisplayName;
            public uint dwReserved1;
            public uint dwIntVersion;
            public IntPtr hProv;
            public CRYPT_ALGORITHM_IDENTIFIER DigestAlgorithm;
            public uint dwFlags;
            public uint dwEncodingType;
            public uint dwReserved2;
            public uint fdwCAPISettings;
            public uint fdwSecuritySettings;
            public uint dwIndex;
            public uint dwUnionChoice;
            public IntPtr psFlat;
            public IntPtr pClientData;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CERT_CONTEXT {
            public uint dwCertEncodingType;
            public IntPtr pbCertEncoded;
            public uint cbCertEncoded;
            public IntPtr pCertInfo;
            public IntPtr hCertStore;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CERT_TRUST_STATUS {
            public uint dwErrorStatus;
            public uint dwInfoStatus;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CERT_CHAIN_CONTEXT_HEADER {
            public uint cbSize;
            public CERT_TRUST_STATUS TrustStatus;
            public uint cChain;
            public IntPtr rgpChain;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CERT_SIMPLE_CHAIN_HEADER {
            public uint cbSize;
            public CERT_TRUST_STATUS TrustStatus;
            public uint cElement;
            public IntPtr rgpElement;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CERT_CHAIN_ELEMENT_HEADER {
            public uint cbSize;
            public IntPtr pCertContext;
            public CERT_TRUST_STATUS TrustStatus;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CERT_ENHKEY_USAGE {
            public uint cUsageIdentifier;
            public IntPtr rgpszUsageIdentifier;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CERT_USAGE_MATCH {
            public uint dwType;
            public CERT_ENHKEY_USAGE Usage;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CERT_CHAIN_PARA {
            public uint cbSize;
            public CERT_USAGE_MATCH RequestedUsage;
            public CERT_USAGE_MATCH RequestedIssuancePolicy;
            public uint dwUrlRetrievalTimeout;
            public int fCheckRevocationFreshnessTime;
            public uint dwRevocationFreshnessTime;
            public IntPtr pftCacheResync;
            public IntPtr pStrongSignPara;
            public uint dwStrongSignFlags;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CERT_CHAIN_ENGINE_CONFIG {
            public uint cbSize;
            public IntPtr hRestrictedRoot;
            public IntPtr hRestrictedTrust;
            public IntPtr hRestrictedOther;
            public uint cAdditionalStore;
            public IntPtr rghAdditionalStore;
            public uint dwFlags;
            public uint dwUrlRetrievalTimeout;
            public uint MaximumCachedCertificates;
            public uint CycleDetectionModulus;
            public IntPtr hExclusiveRoot;
            public IntPtr hExclusiveTrustedPeople;
            public uint dwExclusiveFlags;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CERT_CHAIN_POLICY_PARA {
            public uint cbSize;
            public uint dwFlags;
            public IntPtr pvExtraPolicyPara;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CERT_CHAIN_POLICY_STATUS {
            public uint cbSize;
            public uint dwError;
            public int lChainIndex;
            public int lElementIndex;
            public IntPtr pvExtraPolicyStatus;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct WINHTTP_PROXY_INFO {
            public uint dwAccessType;
            public IntPtr lpszProxy;
            public IntPtr lpszProxyBypass;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct WINHTTP_CURRENT_USER_IE_PROXY_CONFIG {
            [MarshalAs(UnmanagedType.Bool)] public bool fAutoDetect;
            public IntPtr lpszAutoConfigUrl;
            public IntPtr lpszProxy;
            public IntPtr lpszProxyBypass;
        }

        [DllImport("wintrust.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
        private static extern int WinVerifyTrust(IntPtr hwnd, [In] ref Guid actionId, [In, Out] ref WINTRUST_DATA data);
        [DllImport("wintrust.dll", ExactSpelling = true)]
        private static extern IntPtr WTHelperProvDataFromStateData(IntPtr stateData);
        [DllImport("wintrust.dll", ExactSpelling = true)]
        private static extern IntPtr WTHelperGetProvSignerFromChain(IntPtr providerData, uint signerIndex, [MarshalAs(UnmanagedType.Bool)] bool counterSigner, uint counterSignerIndex);
        [DllImport("wintrust.dll", ExactSpelling = true)]
        private static extern IntPtr WTHelperGetProvCertFromChain(IntPtr signer, uint certIndex);

        [DllImport("crypt32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
        private static extern IntPtr CertOpenStore(IntPtr storeProvider, uint encodingType, IntPtr cryptProv, uint flags, IntPtr parameters);
        [DllImport("crypt32.dll", SetLastError = true)]
        private static extern bool CertCloseStore(IntPtr certStore, uint flags);
        [DllImport("crypt32.dll", SetLastError = true)]
        private static extern bool CertAddEncodedCertificateToStore(IntPtr certStore, uint encodingType, byte[] encoded, uint encodedLength, uint disposition, out IntPtr certContext);
        [DllImport("crypt32.dll", SetLastError = true)]
        private static extern bool CertFreeCertificateContext(IntPtr certContext);
        [DllImport("crypt32.dll", SetLastError = true)]
        private static extern bool CertCreateCertificateChainEngine(ref CERT_CHAIN_ENGINE_CONFIG config, out IntPtr chainEngine);
        [DllImport("crypt32.dll")]
        private static extern void CertFreeCertificateChainEngine(IntPtr chainEngine);
        [DllImport("crypt32.dll", SetLastError = true)]
        private static extern bool CertGetCertificateChain(IntPtr chainEngine, IntPtr certContext, IntPtr time, IntPtr additionalStore, ref CERT_CHAIN_PARA chainPara, uint flags, IntPtr reserved, out IntPtr chainContext);
        [DllImport("crypt32.dll")]
        private static extern void CertFreeCertificateChain(IntPtr chainContext);
        [DllImport("crypt32.dll", SetLastError = true)]
        private static extern bool CertVerifyCertificateChainPolicy(IntPtr policyOid, IntPtr chainContext, ref CERT_CHAIN_POLICY_PARA policyPara, ref CERT_CHAIN_POLICY_STATUS policyStatus);
        [DllImport("crypt32.dll", SetLastError = true)]
        private static extern bool CryptVerifyCertificateSignatureEx(IntPtr cryptProv, uint encodingType, uint subjectType, IntPtr subject, uint issuerType, IntPtr issuer, uint flags, IntPtr extra);
        [DllImport("crypt32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool CryptSIPRetrieveSubjectGuid(string fileName, IntPtr file, out Guid subjectGuid);
        [DllImport("crypt32.dll", SetLastError = true)]
        private static extern bool CryptSIPGetSignedDataMsg(ref SIP_SUBJECTINFO subjectInfo, out uint encodingType, uint index, ref uint signedDataSize, byte[] signedData);
        [DllImport("crypt32.dll", SetLastError = true)]
        private static extern bool CryptSIPVerifyIndirectData(ref SIP_SUBJECTINFO subjectInfo, IntPtr indirectData);
        [DllImport("crypt32.dll", SetLastError = true)]
        private static extern bool CryptDecodeObjectEx(uint encodingType, IntPtr structType, byte[] encoded, uint encodedSize, uint flags, IntPtr decodeParameters, IntPtr decoded, ref uint decodedSize);
        [DllImport("winhttp.dll", SetLastError = true)]
        private static extern bool WinHttpGetDefaultProxyConfiguration(out WINHTTP_PROXY_INFO proxyInfo);
        [DllImport("winhttp.dll", SetLastError = true)]
        private static extern bool WinHttpGetIEProxyConfigForCurrentUser(out WINHTTP_CURRENT_USER_IE_PROXY_CONFIG proxyConfig);
        [DllImport("kernel32.dll", ExactSpelling = true)]
        private static extern IntPtr GlobalFree(IntPtr memory);

        public static FileTrustEvidence VerifyOnlineFileTrust(string filePath) {
            return VerifyOnlineFileTrustCore(filePath);
        }

        public static FileTrustEvidence VerifyOfflineFileDigestAndSignature(string filePath) {
            return VerifyOfflineSipSignature(filePath);
        }

        private static FileTrustEvidence VerifyOfflineSipSignature(string filePath) {
            if (String.IsNullOrWhiteSpace(filePath)) throw new ArgumentException("File path is required.", "filePath");
            string canonical = System.IO.Path.GetFullPath(filePath);
            FileTrustEvidence evidence = new FileTrustEvidence {
                hashOnly = true,
                cacheOnlyUrlRetrieval = true
            };
            try {
                int messageCount = 0;
                int signerCount = 0;
                int timestampSignerCount = 0;
                for (uint index = 0; index < 8; index++) {
                    uint encodingType;
                    SIP_SUBJECTINFO subject;
                    byte[] signedMessage = GetSipSignedMessage(canonical, index, out encodingType, out subject);
                    if (signedMessage == null) break;
                    messageCount++;
                    try {
                        SignedCms cms = new SignedCms();
                        cms.Decode(signedMessage);
                        if (!String.Equals(cms.ContentInfo.ContentType.Value, "1.3.6.1.4.1.311.2.1.4", StringComparison.Ordinal)) {
                            throw new CryptographicException("Signed content is not SPC indirect data.");
                        }
                        foreach (SignerInfo signer in cms.SignerInfos) {
                            signerCount++;
                            signer.CheckSignature(true);
                            if (signer.Certificate == null) throw new CryptographicException("Signer certificate is missing.");
                            if (signerCount == 1) evidence.signerLeafDerSha256 = Sha256(signer.Certificate.RawData);
                            foreach (SignerInfo timestampSigner in signer.CounterSignerInfos) {
                                timestampSignerCount++;
                                timestampSigner.CheckSignature(true);
                                if (timestampSigner.Certificate == null) throw new CryptographicException("Timestamp signer certificate is missing.");
                                if (timestampSignerCount == 1) {
                                    evidence.timestampLeafDerSha256 = Sha256(timestampSigner.Certificate.RawData);
                                    evidence.timestampUtc = GetCmsSigningTime(timestampSigner);
                                }
                            }
                        }

                        VerifySipIndirectData(ref subject, encodingType, cms.ContentInfo.Content);
                    }
                    finally {
                        FreeSipSubject(ref subject);
                    }
                }

                evidence.signerCount = signerCount;
                evidence.timestampSignerCount = timestampSignerCount;
                evidence.providerSignerError = 0;
                evidence.providerTimestampError = 0;
                if (messageCount != 1 || signerCount != 1 ||
                    String.IsNullOrWhiteSpace(evidence.signerLeafDerSha256) ||
                    (timestampSignerCount > 0 && (String.IsNullOrWhiteSpace(evidence.timestampLeafDerSha256) || String.IsNullOrWhiteSpace(evidence.timestampUtc)))) {
                    throw new CryptographicException("The embedded signer or timestamp countersigner set is invalid.");
                }
                evidence.status = 0;
                evidence.statusHex = Hex(0);
                evidence.passed = true;
                return evidence;
            }
            catch (CryptographicException exception) {
                evidence.status = exception.HResult;
                evidence.statusHex = Hex(exception.HResult);
                evidence.failureOperation = "managed-signature-validation";
                evidence.passed = false;
                return evidence;
            }
            catch (Win32Exception exception) {
                evidence.status = exception.NativeErrorCode;
                evidence.statusHex = Hex(exception.NativeErrorCode);
                evidence.failureOperation = exception.Message.Split(' ')[0];
                evidence.passed = false;
                return evidence;
            }
        }

        private static byte[] GetSipSignedMessage(string filePath, uint index, out uint encodingType, out SIP_SUBJECTINFO subject) {
            encodingType = 0;
            subject = new SIP_SUBJECTINFO();
            Guid subjectGuid;
            if (!CryptSIPRetrieveSubjectGuid(filePath, IntPtr.Zero, out subjectGuid)) Win32("CryptSIPRetrieveSubjectGuid");
            IntPtr subjectGuidPointer = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(Guid)));
            bool retained = false;
            try {
                Marshal.StructureToPtr(subjectGuid, subjectGuidPointer, false);
                subject = new SIP_SUBJECTINFO {
                    cbSize = (uint)Marshal.SizeOf(typeof(SIP_SUBJECTINFO)),
                    pgSubjectType = subjectGuidPointer,
                    hFile = new IntPtr(-1),
                    pwsFileName = filePath,
                    pwsDisplayName = filePath,
                    dwIndex = index
                };
                uint size = 0;
                bool measured = CryptSIPGetSignedDataMsg(ref subject, out encodingType, index, ref size, null);
                if (!measured) {
                    uint error = unchecked((uint)Marshal.GetLastWin32Error());
                    if (error == CRYPT_E_NO_MATCH || (index > 0 && error == ERROR_INVALID_PARAMETER)) return null;
                    if (size == 0 || (error != ERROR_INSUFFICIENT_BUFFER && error != ERROR_MORE_DATA)) Win32("CryptSIPGetSignedDataMsg(size)");
                }
                if (size == 0 || size > 16 * 1024 * 1024) throw new CryptographicException("Embedded signature size is invalid.");
                byte[] message = new byte[checked((int)size)];
                if (!CryptSIPGetSignedDataMsg(ref subject, out encodingType, index, ref size, message)) Win32("CryptSIPGetSignedDataMsg");
                if (size != message.Length) Array.Resize(ref message, checked((int)size));
                retained = true;
                return message;
            }
            finally {
                if (!retained) {
                    Marshal.FreeHGlobal(subjectGuidPointer);
                    subject.pgSubjectType = IntPtr.Zero;
                }
            }
        }

        private static void VerifySipIndirectData(ref SIP_SUBJECTINFO subject, uint encodingType, byte[] encodedIndirectData) {
            IntPtr decoded = IntPtr.Zero;
            try {
                uint decodedSize = 0;
                uint combinedEncoding = encodingType | X509_ASN_ENCODING | PKCS_7_ASN_ENCODING;
                if (!CryptDecodeObjectEx(combinedEncoding, new IntPtr(SPC_INDIRECT_DATA_CONTENT_STRUCT), encodedIndirectData, (uint)encodedIndirectData.Length, 0, IntPtr.Zero, IntPtr.Zero, ref decodedSize)) {
                    Win32("CryptDecodeObjectEx(size)");
                }
                if (decodedSize == 0 || decodedSize > 1024 * 1024) throw new CryptographicException("Decoded indirect data size is invalid.");
                decoded = Marshal.AllocHGlobal(checked((int)decodedSize));
                if (!CryptDecodeObjectEx(combinedEncoding, new IntPtr(SPC_INDIRECT_DATA_CONTENT_STRUCT), encodedIndirectData, (uint)encodedIndirectData.Length, 0, IntPtr.Zero, decoded, ref decodedSize)) {
                    Win32("CryptDecodeObjectEx");
                }
                subject.dwEncodingType = encodingType;
                if (!CryptSIPVerifyIndirectData(ref subject, decoded)) Win32("CryptSIPVerifyIndirectData");
            }
            finally {
                if (decoded != IntPtr.Zero) Marshal.FreeHGlobal(decoded);
            }
        }

        private static void FreeSipSubject(ref SIP_SUBJECTINFO subject) {
            if (subject.pgSubjectType == IntPtr.Zero) return;
            Marshal.FreeHGlobal(subject.pgSubjectType);
            subject.pgSubjectType = IntPtr.Zero;
        }

        private static string GetCmsSigningTime(SignerInfo signer) {
            List<DateTime> values = new List<DateTime>();
            foreach (CryptographicAttributeObject attribute in signer.SignedAttributes) {
                if (!String.Equals(attribute.Oid.Value, "1.2.840.113549.1.9.5", StringComparison.Ordinal)) continue;
                foreach (AsnEncodedData value in attribute.Values) values.Add(new Pkcs9SigningTime(value.RawData).SigningTime.ToUniversalTime());
            }
            if (values.Count != 1) throw new CryptographicException("Timestamp signing time is missing or ambiguous.");
            return values[0].ToString("o", CultureInfo.InvariantCulture);
        }

        public static ProxyConfigurationEvidence GetProxyConfiguration() {
            WINHTTP_PROXY_INFO system;
            if (!WinHttpGetDefaultProxyConfiguration(out system)) Win32("WinHttpGetDefaultProxyConfiguration");
            WINHTTP_CURRENT_USER_IE_PROXY_CONFIG user;
            try {
                if (!WinHttpGetIEProxyConfigForCurrentUser(out user)) Win32("WinHttpGetIEProxyConfigForCurrentUser");
                try {
                    bool systemNoProxy = system.dwAccessType == WINHTTP_ACCESS_TYPE_NO_PROXY && system.lpszProxy == IntPtr.Zero;
                    bool autoConfig = user.lpszAutoConfigUrl != IntPtr.Zero;
                    bool namedProxy = user.lpszProxy != IntPtr.Zero;
                    return new ProxyConfigurationEvidence {
                        systemNoProxy = systemNoProxy,
                        userAutoDetect = user.fAutoDetect,
                        userAutoConfigUrlPresent = autoConfig,
                        userNamedProxyPresent = namedProxy,
                        proxyFree = systemNoProxy && !autoConfig && !namedProxy
                    };
                }
                finally {
                    FreeGlobal(user.lpszAutoConfigUrl);
                    FreeGlobal(user.lpszProxy);
                    FreeGlobal(user.lpszProxyBypass);
                }
            }
            finally {
                FreeGlobal(system.lpszProxy);
                FreeGlobal(system.lpszProxyBypass);
            }
        }

        private static FileTrustEvidence VerifyOnlineFileTrustCore(string filePath) {
            if (String.IsNullOrWhiteSpace(filePath)) throw new ArgumentException("File path is required.", "filePath");
            string canonical = System.IO.Path.GetFullPath(filePath);
            WINTRUST_FILE_INFO file = new WINTRUST_FILE_INFO {
                cbStruct = (uint)Marshal.SizeOf(typeof(WINTRUST_FILE_INFO)),
                pcwszFilePath = canonical,
                hFile = IntPtr.Zero,
                pgKnownSubject = IntPtr.Zero
            };
            IntPtr filePointer = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(WINTRUST_FILE_INFO)));
            Marshal.StructureToPtr(file, filePointer, false);
            WINTRUST_DATA data = new WINTRUST_DATA {
                cbStruct = (uint)Marshal.SizeOf(typeof(WINTRUST_DATA)),
                dwUIChoice = WTD_UI_NONE,
                fdwRevocationChecks = WTD_REVOKE_WHOLECHAIN,
                dwUnionChoice = WTD_CHOICE_FILE,
                pFile = filePointer,
                dwStateAction = WTD_STATEACTION_VERIFY,
                dwProvFlags = WTD_DISABLE_MD2_MD4 | WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT
            };
            int status = unchecked((int)0x800B0001);
            FileTrustEvidence evidence = new FileTrustEvidence {
                hashOnly = false,
                cacheOnlyUrlRetrieval = false
            };
            try {
                Guid action = GenericVerifyV2;
                status = WinVerifyTrust(IntPtr.Zero, ref action, ref data);
                evidence.status = status;
                evidence.statusHex = Hex(status);
                evidence.passed = status == 0;
                IntPtr provider = WTHelperProvDataFromStateData(data.hWVTStateData);
                if (provider == IntPtr.Zero) return evidence;

                List<IntPtr> signers = new List<IntPtr>();
                for (uint index = 0; index < 8; index++) {
                    IntPtr signer = WTHelperGetProvSignerFromChain(provider, index, false, 0);
                    if (signer == IntPtr.Zero) break;
                    signers.Add(signer);
                }
                evidence.signerCount = signers.Count;
                if (signers.Count > 0) {
                    CRYPT_PROVIDER_SGNR signer = (CRYPT_PROVIDER_SGNR)Marshal.PtrToStructure(signers[0], typeof(CRYPT_PROVIDER_SGNR));
                    evidence.providerSignerError = unchecked((int)signer.dwError);
                    evidence.signerLeafDerSha256 = ProviderLeafSha256(signers[0]);
                    List<IntPtr> timestamps = new List<IntPtr>();
                    for (uint index = 0; index < 8; index++) {
                        IntPtr timestamp = WTHelperGetProvSignerFromChain(provider, 0, true, index);
                        if (timestamp == IntPtr.Zero) break;
                        timestamps.Add(timestamp);
                    }
                    evidence.timestampSignerCount = timestamps.Count;
                    if (timestamps.Count > 0) {
                        CRYPT_PROVIDER_SGNR timestamp = (CRYPT_PROVIDER_SGNR)Marshal.PtrToStructure(timestamps[0], typeof(CRYPT_PROVIDER_SGNR));
                        evidence.providerTimestampError = unchecked((int)timestamp.dwError);
                        evidence.timestampLeafDerSha256 = ProviderLeafSha256(timestamps[0]);
                        evidence.timestampUtc = FileTimeText(timestamp.sftVerifyAsOf);
                    } else {
                        evidence.timestampUtc = FileTimeText(signer.sftVerifyAsOf);
                    }
                }
                return evidence;
            }
            finally {
                if (data.hWVTStateData != IntPtr.Zero) {
                    data.dwStateAction = WTD_STATEACTION_CLOSE;
                    Guid closeAction = GenericVerifyV2;
                    WinVerifyTrust(IntPtr.Zero, ref closeAction, ref data);
                }
                Marshal.DestroyStructure(filePointer, typeof(WINTRUST_FILE_INFO));
                Marshal.FreeHGlobal(filePointer);
            }
        }

        public static ChainEvidence BuildOnlineChain(byte[] leafDer, string purpose, string verificationTimeUtc, int timeoutMilliseconds) {
            if (timeoutMilliseconds < 1000 || timeoutMilliseconds > 120000) throw new ArgumentOutOfRangeException("timeoutMilliseconds");
            return BuildChain(new byte[][] { leafDer }, purpose, verificationTimeUtc, true, false, timeoutMilliseconds);
        }

        public static ChainEvidence BuildOfflineExclusiveChain(byte[][] orderedChainDer, string purpose, string verificationTimeUtc) {
            if (orderedChainDer == null || orderedChainDer.Length < 2) throw new ArgumentException("An ordered leaf-to-root chain is required.", "orderedChainDer");
            return BuildChain(orderedChainDer, purpose, verificationTimeUtc, false, true, 1000);
        }

        private static ChainEvidence BuildChain(byte[][] inputDer, string purpose, string verificationTimeUtc, bool online, bool exclusiveRoot, int timeoutMilliseconds) {
            if (inputDer == null || inputDer.Length == 0 || inputDer.Any(value => value == null || value.Length == 0)) throw new ArgumentException("Certificate DER is required.", "inputDer");
            bool timestampPurpose = String.Equals(purpose, "timestamp", StringComparison.Ordinal);
            if (!timestampPurpose && !String.Equals(purpose, "signer", StringComparison.Ordinal)) throw new ArgumentException("Purpose must be signer or timestamp.", "purpose");
            DateTime verificationTime = DateTime.Parse(verificationTimeUtc, CultureInfo.InvariantCulture, DateTimeStyles.AdjustToUniversal | DateTimeStyles.AssumeUniversal).ToUniversalTime();

            IntPtr store = IntPtr.Zero;
            IntPtr roots = IntPtr.Zero;
            IntPtr leaf = IntPtr.Zero;
            IntPtr engine = IntPtr.Zero;
            IntPtr chain = IntPtr.Zero;
            IntPtr oid = IntPtr.Zero;
            IntPtr oidArray = IntPtr.Zero;
            IntPtr fileTime = IntPtr.Zero;
            List<IntPtr> contexts = new List<IntPtr>();
            try {
                store = OpenMemoryStore();
                if (exclusiveRoot) roots = OpenMemoryStore();
                for (int index = 0; index < inputDer.Length; index++) {
                    IntPtr context;
                    if (!CertAddEncodedCertificateToStore(store, X509_ASN_ENCODING, inputDer[index], (uint)inputDer[index].Length, CERT_STORE_ADD_ALWAYS, out context)) Win32("CertAddEncodedCertificateToStore");
                    contexts.Add(context);
                    if (index == 0) leaf = context;
                    if (exclusiveRoot && index == inputDer.Length - 1) {
                        IntPtr rootContext;
                        if (!CertAddEncodedCertificateToStore(roots, X509_ASN_ENCODING, inputDer[index], (uint)inputDer[index].Length, CERT_STORE_ADD_ALWAYS, out rootContext)) Win32("CertAddEncodedCertificateToStore(root)");
                        contexts.Add(rootContext);
                    }
                }

                if (exclusiveRoot) {
                    CERT_CHAIN_ENGINE_CONFIG config = new CERT_CHAIN_ENGINE_CONFIG {
                        cbSize = (uint)Marshal.SizeOf(typeof(CERT_CHAIN_ENGINE_CONFIG)),
                        dwUrlRetrievalTimeout = 1000,
                        hExclusiveRoot = roots,
                        dwExclusiveFlags = 0
                    };
                    if (!CertCreateCertificateChainEngine(ref config, out engine)) Win32("CertCreateCertificateChainEngine");
                }

                string usageOid = timestampPurpose ? "1.3.6.1.5.5.7.3.8" : "1.3.6.1.5.5.7.3.3";
                CERT_USAGE_MATCH requestedUsage = new CERT_USAGE_MATCH();
                if (!timestampPurpose) {
                    oid = Marshal.StringToHGlobalAnsi(usageOid);
                    oidArray = Marshal.AllocHGlobal(IntPtr.Size);
                    Marshal.WriteIntPtr(oidArray, oid);
                    requestedUsage = new CERT_USAGE_MATCH {
                        dwType = 0,
                        Usage = new CERT_ENHKEY_USAGE { cUsageIdentifier = 1, rgpszUsageIdentifier = oidArray }
                    };
                }
                CERT_CHAIN_PARA chainPara = new CERT_CHAIN_PARA {
                    cbSize = (uint)Marshal.SizeOf(typeof(CERT_CHAIN_PARA)),
                    // AUTHENTICODE_TS evaluates timestamp purpose. Supplying a timestamp
                    // RequestedUsage makes Windows mark this valid Certum TSA chain WrongUsage.
                    RequestedUsage = requestedUsage,
                    dwUrlRetrievalTimeout = (uint)timeoutMilliseconds
                };
                long ticks = verificationTime.ToFileTimeUtc();
                FILETIME_NATIVE nativeTime = new FILETIME_NATIVE { dwLowDateTime = (uint)(ticks & 0xffffffff), dwHighDateTime = (uint)(ticks >> 32) };
                fileTime = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(FILETIME_NATIVE)));
                Marshal.StructureToPtr(nativeTime, fileTime, false);
                uint flags = CERT_CHAIN_TIMESTAMP_TIME;
                if (online) flags |= CERT_CHAIN_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT | CERT_CHAIN_REVOCATION_ACCUMULATIVE_TIMEOUT;
                else flags |= CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL | CERT_CHAIN_DISABLE_AUTH_ROOT_AUTO_UPDATE | CERT_CHAIN_DISABLE_AIA;
                if (!CertGetCertificateChain(engine, leaf, fileTime, store, ref chainPara, flags, IntPtr.Zero, out chain)) Win32("CertGetCertificateChain");

                CERT_CHAIN_CONTEXT_HEADER header = (CERT_CHAIN_CONTEXT_HEADER)Marshal.PtrToStructure(chain, typeof(CERT_CHAIN_CONTEXT_HEADER));
                if (header.cChain != 1) throw new InvalidOperationException("Certificate chain builder returned an ambiguous chain set.");
                IntPtr simplePointer = Marshal.ReadIntPtr(header.rgpChain);
                CERT_SIMPLE_CHAIN_HEADER simple = (CERT_SIMPLE_CHAIN_HEADER)Marshal.PtrToStructure(simplePointer, typeof(CERT_SIMPLE_CHAIN_HEADER));
                List<CertificateEvidence> certificates = new List<CertificateEvidence>();
                List<IntPtr> chainCertificateContexts = new List<IntPtr>();
                for (int index = 0; index < checked((int)simple.cElement); index++) {
                    IntPtr elementPointer = Marshal.ReadIntPtr(simple.rgpElement, index * IntPtr.Size);
                    CERT_CHAIN_ELEMENT_HEADER element = (CERT_CHAIN_ELEMENT_HEADER)Marshal.PtrToStructure(elementPointer, typeof(CERT_CHAIN_ELEMENT_HEADER));
                    chainCertificateContexts.Add(element.pCertContext);
                    certificates.Add(CertificateFromContext(element.pCertContext, index));
                }
                bool certificateSignaturesValid = true;
                for (int index = 0; index < chainCertificateContexts.Count; index++) {
                    IntPtr issuer = index + 1 < chainCertificateContexts.Count ? chainCertificateContexts[index + 1] : chainCertificateContexts[index];
                    if (!CryptVerifyCertificateSignatureEx(IntPtr.Zero, X509_ASN_ENCODING, CRYPT_VERIFY_CERT_SIGN_SUBJECT_CERT, chainCertificateContexts[index],
                        CRYPT_VERIFY_CERT_SIGN_ISSUER_CERT, issuer, CRYPT_VERIFY_CERT_SIGN_DISABLE_MD2_MD4_FLAG, IntPtr.Zero)) {
                        certificateSignaturesValid = false;
                        break;
                    }
                }

                CERT_CHAIN_POLICY_PARA policyPara = new CERT_CHAIN_POLICY_PARA { cbSize = (uint)Marshal.SizeOf(typeof(CERT_CHAIN_POLICY_PARA)) };
                CERT_CHAIN_POLICY_STATUS policyStatus = new CERT_CHAIN_POLICY_STATUS { cbSize = (uint)Marshal.SizeOf(typeof(CERT_CHAIN_POLICY_STATUS)), lChainIndex = -1, lElementIndex = -1 };
                IntPtr policy = timestampPurpose ? CERT_CHAIN_POLICY_AUTHENTICODE_TS : CERT_CHAIN_POLICY_AUTHENTICODE;
                if (!CertVerifyCertificateChainPolicy(policy, chain, ref policyPara, ref policyStatus)) Win32("CertVerifyCertificateChainPolicy");
                uint errors = header.TrustStatus.dwErrorStatus | simple.TrustStatus.dwErrorStatus;
                string[] names = TrustStatusNames(errors);
                bool complete = (errors & TRUST_IS_PARTIAL_CHAIN) == 0 && certificates.Count >= 2;
                bool trusted = (errors & TRUST_IS_UNTRUSTED_ROOT) == 0;
                bool purposeEkuValid = PurposeEkuValid(certificates, usageOid);
                bool revocationGood = online && (errors & (TRUST_IS_REVOKED | TRUST_REVOCATION_STATUS_UNKNOWN | TRUST_IS_OFFLINE_REVOCATION)) == 0;
                bool valid = errors == 0 && policyStatus.dwError == 0 && complete && trusted && certificateSignaturesValid && purposeEkuValid && (online ? revocationGood : true);
                return new ChainEvidence {
                    purpose = purpose,
                    onlineRevocation = online,
                    exclusiveRoot = exclusiveRoot,
                    urlRetrievalDisabled = !online,
                    errorStatus = unchecked((int)errors),
                    errorStatusHex = Hex(unchecked((int)errors)),
                    errorStatusNames = names,
                    infoStatus = unchecked((int)(header.TrustStatus.dwInfoStatus | simple.TrustStatus.dwInfoStatus)),
                    policyError = unchecked((int)policyStatus.dwError),
                    policyErrorHex = Hex(unchecked((int)policyStatus.dwError)),
                    policyChainIndex = policyStatus.lChainIndex,
                    policyElementIndex = policyStatus.lElementIndex,
                    complete = complete,
                    trustedRoot = trusted,
                    certificateSignaturesValid = certificateSignaturesValid,
                    purposeEkuValid = purposeEkuValid,
                    revocationKnownGood = revocationGood,
                    valid = valid,
                    verificationTimeUtc = verificationTime.ToString("o", CultureInfo.InvariantCulture),
                    certificates = certificates.ToArray()
                };
            }
            finally {
                if (chain != IntPtr.Zero) CertFreeCertificateChain(chain);
                if (engine != IntPtr.Zero) CertFreeCertificateChainEngine(engine);
                foreach (IntPtr context in contexts) if (context != IntPtr.Zero) CertFreeCertificateContext(context);
                if (store != IntPtr.Zero) CertCloseStore(store, 0);
                if (roots != IntPtr.Zero) CertCloseStore(roots, 0);
                if (fileTime != IntPtr.Zero) Marshal.FreeHGlobal(fileTime);
                if (oidArray != IntPtr.Zero) Marshal.FreeHGlobal(oidArray);
                if (oid != IntPtr.Zero) Marshal.FreeHGlobal(oid);
            }
        }

        public static CertificateEvidence GetCertificateEvidence(byte[] der, int chainPosition) {
            if (der == null || der.Length == 0) throw new ArgumentException("Certificate DER is required.", "der");
            return CertificateFromDer(der, chainPosition);
        }

        private static bool PurposeEkuValid(List<CertificateEvidence> certificates, string requiredOid) {
            if (certificates == null || certificates.Count < 2) return false;
            if (certificates[0].ekuOids == null || !certificates[0].ekuOids.Contains(requiredOid, StringComparer.Ordinal)) return false;
            for (int index = 1; index < certificates.Count; index++) {
                string[] eku = certificates[index].ekuOids ?? new string[0];
                if (eku.Length > 0 && !eku.Contains(requiredOid, StringComparer.Ordinal)) return false;
            }
            return true;
        }

        private static IntPtr OpenMemoryStore() {
            IntPtr store = CertOpenStore(new IntPtr(2), 0, IntPtr.Zero, CERT_STORE_CREATE_NEW_FLAG, IntPtr.Zero);
            if (store == IntPtr.Zero) Win32("CertOpenStore(memory)");
            return store;
        }

        private static CertificateEvidence CertificateFromContext(IntPtr contextPointer, int position) {
            CERT_CONTEXT context = (CERT_CONTEXT)Marshal.PtrToStructure(contextPointer, typeof(CERT_CONTEXT));
            byte[] der = new byte[context.cbCertEncoded];
            Marshal.Copy(context.pbCertEncoded, der, 0, der.Length);
            return CertificateFromDer(der, position);
        }

        private static CertificateEvidence CertificateFromDer(byte[] der, int position) {
            using (X509Certificate2 certificate = new X509Certificate2(der)) {
                List<string> eku = new List<string>();
                foreach (X509Extension extension in certificate.Extensions) {
                    X509EnhancedKeyUsageExtension usages = extension as X509EnhancedKeyUsageExtension;
                    if (usages != null) foreach (Oid oid in usages.EnhancedKeyUsages) eku.Add(oid.Value);
                }
                int keySize = 0;
                try { using (AsymmetricAlgorithm key = certificate.PublicKey.Key) { if (key != null) keySize = key.KeySize; } } catch (CryptographicException) { }
                return new CertificateEvidence {
                    chainPosition = position,
                    derSha256 = Sha256(der),
                    thumbprint = certificate.Thumbprint.ToUpperInvariant(),
                    serialNumber = certificate.SerialNumber.ToUpperInvariant(),
                    subjectNameSha256 = Sha256(certificate.SubjectName.RawData),
                    issuerNameSha256 = Sha256(certificate.IssuerName.RawData),
                    publicKeyAlgorithmOid = certificate.PublicKey.Oid.Value,
                    publicKeySizeBits = keySize,
                    signatureAlgorithmOid = certificate.SignatureAlgorithm.Value,
                    ekuOids = eku.OrderBy(value => value, StringComparer.Ordinal).ToArray(),
                    notBeforeUtc = certificate.NotBefore.ToUniversalTime().ToString("o", CultureInfo.InvariantCulture),
                    notAfterUtc = certificate.NotAfter.ToUniversalTime().ToString("o", CultureInfo.InvariantCulture),
                    selfSignedName = certificate.SubjectName.RawData.SequenceEqual(certificate.IssuerName.RawData),
                    der = (byte[])der.Clone()
                };
            }
        }

        private static string ProviderLeafSha256(IntPtr signerPointer) {
            IntPtr providerCertPointer = WTHelperGetProvCertFromChain(signerPointer, 0);
            if (providerCertPointer == IntPtr.Zero) return null;
            CRYPT_PROVIDER_CERT providerCert = (CRYPT_PROVIDER_CERT)Marshal.PtrToStructure(providerCertPointer, typeof(CRYPT_PROVIDER_CERT));
            if (providerCert.pCert == IntPtr.Zero) return null;
            return CertificateFromContext(providerCert.pCert, 0).derSha256;
        }

        private static string FileTimeText(FILETIME_NATIVE fileTime) {
            long value = ((long)fileTime.dwHighDateTime << 32) | fileTime.dwLowDateTime;
            if (value <= 0) return null;
            return DateTime.FromFileTimeUtc(value).ToString("o", CultureInfo.InvariantCulture);
        }

        private static string[] TrustStatusNames(uint status) {
            List<string> names = new List<string>();
            AddStatus(names, status, TRUST_IS_NOT_TIME_VALID, "NotTimeValid");
            AddStatus(names, status, TRUST_IS_REVOKED, "Revoked");
            AddStatus(names, status, TRUST_IS_NOT_SIGNATURE_VALID, "NotSignatureValid");
            AddStatus(names, status, TRUST_IS_NOT_VALID_FOR_USAGE, "WrongUsage");
            AddStatus(names, status, TRUST_IS_UNTRUSTED_ROOT, "UntrustedRoot");
            AddStatus(names, status, TRUST_REVOCATION_STATUS_UNKNOWN, "RevocationStatusUnknown");
            AddStatus(names, status, TRUST_IS_CYCLIC, "Cyclic");
            AddStatus(names, status, TRUST_INVALID_EXTENSION, "InvalidExtension");
            AddStatus(names, status, TRUST_INVALID_POLICY_CONSTRAINTS, "InvalidPolicyConstraints");
            AddStatus(names, status, TRUST_INVALID_BASIC_CONSTRAINTS, "InvalidBasicConstraints");
            AddStatus(names, status, TRUST_INVALID_NAME_CONSTRAINTS, "InvalidNameConstraints");
            AddStatus(names, status, TRUST_HAS_NOT_SUPPORTED_NAME_CONSTRAINT, "UnsupportedNameConstraint");
            AddStatus(names, status, TRUST_HAS_NOT_DEFINED_NAME_CONSTRAINT, "UndefinedNameConstraint");
            AddStatus(names, status, TRUST_HAS_NOT_PERMITTED_NAME_CONSTRAINT, "NotPermittedNameConstraint");
            AddStatus(names, status, TRUST_HAS_EXCLUDED_NAME_CONSTRAINT, "ExcludedNameConstraint");
            AddStatus(names, status, TRUST_IS_PARTIAL_CHAIN, "PartialChain");
            AddStatus(names, status, TRUST_CTL_IS_NOT_TIME_VALID, "CtlNotTimeValid");
            AddStatus(names, status, TRUST_CTL_IS_NOT_SIGNATURE_VALID, "CtlNotSignatureValid");
            AddStatus(names, status, TRUST_CTL_IS_NOT_VALID_FOR_USAGE, "CtlWrongUsage");
            AddStatus(names, status, TRUST_HAS_WEAK_SIGNATURE, "WeakSignature");
            AddStatus(names, status, TRUST_IS_OFFLINE_REVOCATION, "OfflineRevocation");
            AddStatus(names, status, TRUST_NO_ISSUANCE_CHAIN_POLICY, "NoIssuanceChainPolicy");
            AddStatus(names, status, TRUST_IS_EXPLICIT_DISTRUST, "ExplicitDistrust");
            AddStatus(names, status, TRUST_HAS_NOT_SUPPORTED_CRITICAL_EXT, "UnsupportedCriticalExtension");
            return names.ToArray();
        }

        private static void FreeGlobal(IntPtr memory) {
            if (memory != IntPtr.Zero && GlobalFree(memory) != IntPtr.Zero) throw new InvalidOperationException("GlobalFree did not release proxy configuration memory.");
        }

        private static void AddStatus(List<string> names, uint status, uint flag, string name) {
            if ((status & flag) != 0) names.Add(name);
        }

        private static string Sha256(byte[] bytes) {
            using (SHA256 algorithm = SHA256.Create()) return String.Concat(algorithm.ComputeHash(bytes).Select(value => value.ToString("x2", CultureInfo.InvariantCulture)));
        }

        private static string Hex(int value) {
            return "0x" + unchecked((uint)value).ToString("X8", CultureInfo.InvariantCulture);
        }

        private static void Win32(string operation) {
            int error = Marshal.GetLastWin32Error();
            throw new Win32Exception(error, operation + " failed with Win32 error " + error.ToString(CultureInfo.InvariantCulture));
        }
    }
}
