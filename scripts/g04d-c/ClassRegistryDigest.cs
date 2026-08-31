using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Security;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Win32;

namespace DocumentStudio.G04DC
{
    public sealed class DirectClassRegistryDigestCollector
    {
        private const int ErrorSuccess = 0;
        private const int ErrorFileNotFound = 2;
        private const int ErrorAccessDenied = 5;
        private const int ErrorMoreData = 234;
        private static readonly Encoding Utf8 = new UTF8Encoding(false, true);

        private readonly int maximumKeys;
        private readonly int maximumValues;
        private readonly int maximumDepth;
        private readonly int maximumValueBytes;
        private readonly long maximumCanonicalBytes;
        private readonly RegistryNameComparer nameComparer = new RegistryNameComparer();
        private int observedRowCount;
        private long observedRawByteCount;

        public DirectClassRegistryDigestCollector(
            int maximumKeys,
            int maximumValues,
            int maximumDepth,
            int maximumValueBytes,
            long maximumCanonicalBytes)
        {
            if (maximumKeys < 1) throw new ArgumentOutOfRangeException("maximumKeys");
            if (maximumValues < 1) throw new ArgumentOutOfRangeException("maximumValues");
            if (maximumDepth < 1) throw new ArgumentOutOfRangeException("maximumDepth");
            if (maximumValueBytes < 1) throw new ArgumentOutOfRangeException("maximumValueBytes");
            if (maximumCanonicalBytes < 1) throw new ArgumentOutOfRangeException("maximumCanonicalBytes");
            this.maximumKeys = maximumKeys;
            this.maximumValues = maximumValues;
            this.maximumDepth = maximumDepth;
            this.maximumValueBytes = maximumValueBytes;
            this.maximumCanonicalBytes = maximumCanonicalBytes;
        }

        public int SchemaVersion { get { return 2; } }
        public int RowCount { get { return Volatile.Read(ref observedRowCount); } }
        public long RawByteCount { get { return Interlocked.Read(ref observedRawByteCount); } }
        public int KeyCount { get; private set; }
        public int ValueCount { get; private set; }
        public long CanonicalByteCount { get; private set; }
        public long ReadElapsedMilliseconds { get; private set; }
        public long NormalizationElapsedMilliseconds { get; private set; }
        public long CanonicalHashElapsedMilliseconds { get; private set; }
        public string Sha256 { get; private set; }

        public void CollectClassesRoot64(long budgetMilliseconds, Action<long, long> progress)
        {
            Collect(RegistryHive.ClassesRoot, null, budgetMilliseconds, progress);
        }

        public void CollectTestClassesRoot64(string testSubKey, long budgetMilliseconds, Action<long, long> progress)
        {
            if (String.IsNullOrWhiteSpace(testSubKey) ||
                testSubKey.Length > 512 ||
                !testSubKey.StartsWith("DocumentStudioG04DCTest_", StringComparison.Ordinal) ||
                testSubKey.IndexOf('\\') >= 0 ||
                testSubKey.IndexOf('/') >= 0)
            {
                throw new ArgumentException("[REGISTRY_TRAVERSAL_TEST_ROOT_INVALID] Test HKCR root is outside the owned fixture namespace.");
            }
            Collect(RegistryHive.ClassesRoot, testSubKey, budgetMilliseconds, progress);
        }

        private void Collect(RegistryHive hive, string subKey, long budgetMilliseconds, Action<long, long> progress)
        {
            if (budgetMilliseconds < 1) throw new TimeoutException("[REGISTRY_TRAVERSAL_TIMEOUT] No registry traversal budget remained.");
            Stopwatch total = Stopwatch.StartNew();
            try
            {
                PassResult first = CapturePass(hive, subKey, budgetMilliseconds, total, progress);
                PassResult second = CapturePass(hive, subKey, budgetMilliseconds, total, progress);
                if (first.KeyCount != second.KeyCount ||
                    first.ValueCount != second.ValueCount ||
                    first.RawByteCount != second.RawByteCount ||
                    first.CanonicalByteCount != second.CanonicalByteCount ||
                    !String.Equals(first.Sha256, second.Sha256, StringComparison.Ordinal))
                {
                    throw new InvalidDataException("[REGISTRY_TRAVERSAL_UNSTABLE] Consecutive complete registry traversals did not match.");
                }
                KeyCount = first.KeyCount;
                ValueCount = first.ValueCount;
                Volatile.Write(ref observedRowCount, first.RowCount);
                Interlocked.Exchange(ref observedRawByteCount, first.RawByteCount);
                CanonicalByteCount = first.CanonicalByteCount;
                Sha256 = first.Sha256;
            }
            catch (UnauthorizedAccessException exception)
            {
                throw new InvalidDataException("[REGISTRY_TRAVERSAL_ACCESS_DENIED] Registry traversal was denied.", exception);
            }
            catch (SecurityException exception)
            {
                throw new InvalidDataException("[REGISTRY_TRAVERSAL_ACCESS_DENIED] Registry traversal security validation failed.", exception);
            }
            finally
            {
                total.Stop();
            }
        }

        private PassResult CapturePass(
            RegistryHive hive,
            string subKey,
            long budgetMilliseconds,
            Stopwatch total,
            Action<long, long> progress)
        {
            CheckBudget(total, budgetMilliseconds, RowCount);
            using (RegistryKey baseKey = RegistryKey.OpenBaseKey(hive, RegistryView.Registry64))
            {
                RegistryKey root = baseKey;
                bool disposeRoot = false;
                if (!String.IsNullOrEmpty(subKey))
                {
                    root = baseKey.OpenSubKey(subKey, false);
                    disposeRoot = true;
                    if (root == null)
                    {
                        throw new InvalidDataException("[REGISTRY_TRAVERSAL_KEY_DISAPPEARED] The bounded registry root was unavailable.");
                    }
                }
                try
                {
                    CaptureBuffer capture = new CaptureBuffer(maximumCanonicalBytes, Utf8);
                    try
                    {
                        TraverseKey(root, String.Empty, 0, capture, total, budgetMilliseconds, progress);
                        CheckBudget(total, budgetMilliseconds, capture.RowCount);
                        Stopwatch hashStopwatch = Stopwatch.StartNew();
                        string digest;
                        using (SHA256 hash = SHA256.Create())
                        {
                            capture.Stream.Position = 0;
                            digest = ToLowerHex(hash.ComputeHash(capture.Stream));
                        }
                        hashStopwatch.Stop();
                        CanonicalHashElapsedMilliseconds += hashStopwatch.ElapsedMilliseconds;
                        CheckBudget(total, budgetMilliseconds, capture.RowCount);
                        return new PassResult(
                            capture.KeyCount,
                            capture.ValueCount,
                            capture.RawByteCount,
                            capture.Stream.Length,
                            digest);
                    }
                    finally
                    {
                        NormalizationElapsedMilliseconds += capture.NormalizationElapsedMilliseconds;
                        ReadElapsedMilliseconds += capture.ReadElapsedMilliseconds;
                        capture.Dispose();
                    }
                }
                finally
                {
                    if (disposeRoot && root != null) root.Dispose();
                }
            }
        }

        private void TraverseKey(
            RegistryKey key,
            string relativePath,
            int depth,
            CaptureBuffer capture,
            Stopwatch total,
            long budgetMilliseconds,
            Action<long, long> progress)
        {
            if (depth > maximumDepth)
            {
                throw new InvalidDataException("[REGISTRY_TRAVERSAL_DEPTH_CEILING] Registry traversal exceeded the depth ceiling.");
            }
            CheckBudget(total, budgetMilliseconds, capture.RowCount);
            long readStarted = Stopwatch.GetTimestamp();
            string[] valueNames = key.GetValueNames();
            string[] subKeyNames = key.GetSubKeyNames();
            capture.AddReadTicks(Stopwatch.GetTimestamp() - readStarted);
            long sortStarted = Stopwatch.GetTimestamp();
            SortNames(valueNames);
            SortNames(subKeyNames);
            capture.AddNormalizationTicks(Stopwatch.GetTimestamp() - sortStarted);
            capture.AddKey(relativePath, maximumKeys);
            PublishProgress(capture, progress);

            for (int valueIndex = 0; valueIndex < valueNames.Length; valueIndex++)
            {
                CheckBudget(total, budgetMilliseconds, capture.RowCount);
                string valueName = valueNames[valueIndex];
                long valueReadStarted = Stopwatch.GetTimestamp();
                RawRegistryValue rawValue = ReadRawValue(key, valueName);
                capture.AddReadTicks(Stopwatch.GetTimestamp() - valueReadStarted);
                capture.AddValue(relativePath, valueName, rawValue.Type, rawValue.Bytes, maximumValues);
                PublishProgress(capture, progress);
            }

            for (int subKeyIndex = 0; subKeyIndex < subKeyNames.Length; subKeyIndex++)
            {
                CheckBudget(total, budgetMilliseconds, capture.RowCount);
                string name = subKeyNames[subKeyIndex];
                long openStarted = Stopwatch.GetTimestamp();
                RegistryKey child = key.OpenSubKey(name, false);
                capture.AddReadTicks(Stopwatch.GetTimestamp() - openStarted);
                if (child == null)
                {
                    string[] subKeyNamesNow = key.GetSubKeyNames();
                    SortNames(subKeyNamesNow);
                    bool stillListed = false;
                    for (int currentIndex = 0; currentIndex < subKeyNamesNow.Length; currentIndex++)
                    {
                        if (String.Equals(subKeyNamesNow[currentIndex], name, StringComparison.Ordinal))
                        {
                            stillListed = true;
                            break;
                        }
                    }
                    if (stillListed)
                    {
                        throw new InvalidDataException("[REGISTRY_TRAVERSAL_ACCESS_DENIED] A listed registry key could not be opened read-only.");
                    }
                    throw new InvalidDataException("[REGISTRY_TRAVERSAL_KEY_DISAPPEARED] A listed registry key disappeared before it could be read.");
                }
                using (child)
                {
                    string childPath = relativePath.Length == 0 ? name : relativePath + "\\" + name;
                    TraverseKey(child, childPath, depth + 1, capture, total, budgetMilliseconds, progress);
                }
            }

            long verifyStarted = Stopwatch.GetTimestamp();
            string[] valueNamesAfter = key.GetValueNames();
            string[] subKeyNamesAfter = key.GetSubKeyNames();
            capture.AddReadTicks(Stopwatch.GetTimestamp() - verifyStarted);
            long verifySortStarted = Stopwatch.GetTimestamp();
            SortNames(valueNamesAfter);
            SortNames(subKeyNamesAfter);
            capture.AddNormalizationTicks(Stopwatch.GetTimestamp() - verifySortStarted);
            if (!NamesEqual(valueNames, valueNamesAfter) || !NamesEqual(subKeyNames, subKeyNamesAfter))
            {
                throw new InvalidDataException("[REGISTRY_TRAVERSAL_UNSTABLE] Registry membership changed during traversal.");
            }
            CheckBudget(total, budgetMilliseconds, capture.RowCount);
        }

        private RawRegistryValue ReadRawValue(RegistryKey key, string valueName)
        {
            int valueType;
            int size = 0;
            IntPtr nativeHandle = key.Handle.DangerousGetHandle();
            int result = RegQueryValueEx(nativeHandle, valueName, IntPtr.Zero, out valueType, null, ref size);
            if (result == ErrorFileNotFound)
            {
                throw new InvalidDataException("[REGISTRY_TRAVERSAL_VALUE_DISAPPEARED] A listed registry value disappeared before it could be read.");
            }
            if (result == ErrorAccessDenied)
            {
                throw new InvalidDataException("[REGISTRY_TRAVERSAL_ACCESS_DENIED] Registry value access was denied.");
            }
            if (result != ErrorSuccess && result != ErrorMoreData)
            {
                throw new InvalidDataException("[REGISTRY_TRAVERSAL_VALUE_READ_FAILED] Registry value size query failed with a bounded native status.");
            }
            if (size < 0 || size > maximumValueBytes)
            {
                throw new InvalidDataException("[REGISTRY_TRAVERSAL_VALUE_BYTE_CEILING] Registry value exceeded the byte ceiling.");
            }

            for (int attempt = 0; attempt < 4; attempt++)
            {
                byte[] bytes = new byte[size];
                int actualSize = size;
                int actualType;
                result = RegQueryValueEx(nativeHandle, valueName, IntPtr.Zero, out actualType, bytes, ref actualSize);
                if (result == ErrorSuccess)
                {
                    if (actualSize < 0 || actualSize > bytes.Length)
                    {
                        throw new InvalidDataException("[REGISTRY_TRAVERSAL_VALUE_READ_FAILED] Registry value returned an invalid byte count.");
                    }
                    if (actualSize != bytes.Length) Array.Resize(ref bytes, actualSize);
                    return new RawRegistryValue(actualType, bytes);
                }
                if (result == ErrorFileNotFound)
                {
                    throw new InvalidDataException("[REGISTRY_TRAVERSAL_VALUE_DISAPPEARED] A listed registry value disappeared while being read.");
                }
                if (result == ErrorAccessDenied)
                {
                    throw new InvalidDataException("[REGISTRY_TRAVERSAL_ACCESS_DENIED] Registry value access was denied.");
                }
                if (result != ErrorMoreData || actualSize < 0 || actualSize > maximumValueBytes)
                {
                    throw new InvalidDataException("[REGISTRY_TRAVERSAL_VALUE_READ_FAILED] Registry value read failed with a bounded native status.");
                }
                size = actualSize;
            }
            throw new InvalidDataException("[REGISTRY_TRAVERSAL_UNSTABLE] Registry value changed during every bounded read attempt.");
        }

        private void PublishProgress(CaptureBuffer capture, Action<long, long> progress)
        {
            Volatile.Write(ref observedRowCount, capture.RowCount);
            Interlocked.Exchange(ref observedRawByteCount, capture.RawByteCount);
            if (progress != null && (capture.RowCount == 1 || (capture.RowCount & 4095) == 0))
            {
                progress(capture.RowCount, capture.RawByteCount);
            }
        }

        private void SortNames(string[] names)
        {
            Array.Sort(names, nameComparer);
        }

        private static bool NamesEqual(string[] left, string[] right)
        {
            if (left.Length != right.Length) return false;
            for (int index = 0; index < left.Length; index++)
            {
                if (!String.Equals(left[index], right[index], StringComparison.Ordinal)) return false;
            }
            return true;
        }

        private static void CheckBudget(Stopwatch stopwatch, long budgetMilliseconds, int itemCount)
        {
            if (stopwatch.ElapsedMilliseconds > budgetMilliseconds)
            {
                throw new TimeoutException("[REGISTRY_TRAVERSAL_TIMEOUT] Registry traversal exceeded its remaining phase budget; itemCount=" + itemCount.ToString(CultureInfo.InvariantCulture) + ".");
            }
        }

        private static string ToLowerHex(byte[] bytes)
        {
            StringBuilder builder = new StringBuilder(bytes.Length * 2);
            for (int index = 0; index < bytes.Length; index++)
            {
                builder.Append(bytes[index].ToString("x2", CultureInfo.InvariantCulture));
            }
            return builder.ToString();
        }

        [DllImport("advapi32.dll", CharSet = CharSet.Unicode, EntryPoint = "RegQueryValueExW")]
        private static extern int RegQueryValueEx(
            IntPtr key,
            string valueName,
            IntPtr reserved,
            out int type,
            byte[] data,
            ref int dataSize);

        private sealed class RegistryNameComparer : IComparer<string>
        {
            public int Compare(string left, string right)
            {
                int insensitive = StringComparer.OrdinalIgnoreCase.Compare(left, right);
                return insensitive != 0 ? insensitive : StringComparer.Ordinal.Compare(left, right);
            }
        }

        private sealed class RawRegistryValue
        {
            public RawRegistryValue(int type, byte[] bytes)
            {
                Type = type;
                Bytes = bytes;
            }

            public int Type { get; private set; }
            public byte[] Bytes { get; private set; }
        }

        private sealed class PassResult
        {
            public PassResult(int keyCount, int valueCount, long rawByteCount, long canonicalByteCount, string sha256)
            {
                KeyCount = keyCount;
                ValueCount = valueCount;
                RawByteCount = rawByteCount;
                CanonicalByteCount = canonicalByteCount;
                Sha256 = sha256;
            }

            public int KeyCount { get; private set; }
            public int ValueCount { get; private set; }
            public int RowCount { get { return KeyCount + ValueCount; } }
            public long RawByteCount { get; private set; }
            public long CanonicalByteCount { get; private set; }
            public string Sha256 { get; private set; }
        }

        private sealed class CaptureBuffer : IDisposable
        {
            private readonly long maximumBytes;
            private readonly Encoding encoding;
            private long normalizationElapsedTicks;
            private long readElapsedTicks;

            public CaptureBuffer(long maximumBytes, Encoding encoding)
            {
                this.maximumBytes = maximumBytes;
                this.encoding = encoding;
                Stream = new MemoryStream();
            }

            public MemoryStream Stream { get; private set; }
            public int KeyCount { get; private set; }
            public int ValueCount { get; private set; }
            public int RowCount { get { return KeyCount + ValueCount; } }
            public long RawByteCount { get; private set; }
            public long NormalizationElapsedMilliseconds { get { return TicksToMilliseconds(normalizationElapsedTicks); } }
            public long ReadElapsedMilliseconds { get { return TicksToMilliseconds(readElapsedTicks); } }

            public void AddReadTicks(long ticks)
            {
                readElapsedTicks += ticks;
            }

            public void AddNormalizationTicks(long ticks)
            {
                normalizationElapsedTicks += ticks;
            }

            public void AddKey(string path, int maximumKeys)
            {
                if (KeyCount >= maximumKeys)
                {
                    throw new InvalidDataException("[REGISTRY_TRAVERSAL_KEY_CEILING] Registry traversal exceeded the key ceiling.");
                }
                long started = Stopwatch.GetTimestamp();
                WriteByte(1);
                WriteString(path);
                normalizationElapsedTicks += Stopwatch.GetTimestamp() - started;
                KeyCount++;
            }

            public void AddValue(string path, string name, int type, byte[] bytes, int maximumValues)
            {
                if (ValueCount >= maximumValues)
                {
                    throw new InvalidDataException("[REGISTRY_TRAVERSAL_VALUE_CEILING] Registry traversal exceeded the value ceiling.");
                }
                long started = Stopwatch.GetTimestamp();
                WriteByte(2);
                WriteString(path);
                WriteString(name);
                WriteInt32(type);
                WriteInt32(bytes.Length);
                WriteBytes(bytes);
                normalizationElapsedTicks += Stopwatch.GetTimestamp() - started;
                RawByteCount += bytes.Length;
                ValueCount++;
            }

            private void WriteString(string value)
            {
                byte[] bytes = encoding.GetBytes(value);
                WriteInt32(bytes.Length);
                WriteBytes(bytes);
            }

            private void WriteInt32(int value)
            {
                byte[] bytes = BitConverter.GetBytes(value);
                if (!BitConverter.IsLittleEndian) Array.Reverse(bytes);
                WriteBytes(bytes);
            }

            private void WriteByte(byte value)
            {
                EnsureCapacity(1);
                Stream.WriteByte(value);
            }

            private void WriteBytes(byte[] bytes)
            {
                EnsureCapacity(bytes.Length);
                if (bytes.Length != 0) Stream.Write(bytes, 0, bytes.Length);
            }

            private void EnsureCapacity(int additionalBytes)
            {
                if (additionalBytes < 0 || Stream.Length > maximumBytes - additionalBytes)
                {
                    throw new InvalidDataException("[REGISTRY_TRAVERSAL_CANONICAL_BYTE_CEILING] Registry traversal exceeded the canonical-byte ceiling.");
                }
            }

            private static long TicksToMilliseconds(long ticks)
            {
                if (ticks <= 0) return 0;
                return (ticks * 1000L) / Stopwatch.Frequency;
            }

            public void Dispose()
            {
                Stream.Dispose();
            }
        }
    }

    public sealed class ClassRegistryDigestCollector
    {
        private readonly Stream stream;
        private readonly Encoding encoding;
        private readonly long maximumRawBytes;
        private readonly int maximumRows;
        private readonly int maximumRowCharacters;
        private readonly int maximumCanonicalRowBytes;
        private readonly long maximumCanonicalBytes;
        private readonly int readBufferBytes;
        private readonly List<string> rawRows;
        private long rawByteCount;
        private int observedRowCount;
        private long readElapsedMilliseconds;
        private long normalizationElapsedMilliseconds;
        private long canonicalHashElapsedMilliseconds;
        private long canonicalByteCount;
        private string[] canonicalRows;
        private string sha256;
        private bool sawAnyCharacter;
        private bool endedWithLineFeed;

        public ClassRegistryDigestCollector(
            Stream stream,
            Encoding encoding,
            long maximumRawBytes,
            int maximumRows,
            int maximumRowCharacters,
            int maximumCanonicalRowBytes,
            long maximumCanonicalBytes,
            int readBufferBytes)
        {
            if (stream == null) throw new ArgumentNullException("stream");
            if (encoding == null) throw new ArgumentNullException("encoding");
            if (maximumRawBytes < 1) throw new ArgumentOutOfRangeException("maximumRawBytes");
            if (maximumRows < 1) throw new ArgumentOutOfRangeException("maximumRows");
            if (maximumRowCharacters < 1) throw new ArgumentOutOfRangeException("maximumRowCharacters");
            if (maximumCanonicalRowBytes < 1) throw new ArgumentOutOfRangeException("maximumCanonicalRowBytes");
            if (maximumCanonicalBytes < 1) throw new ArgumentOutOfRangeException("maximumCanonicalBytes");
            if (readBufferBytes < 1 || readBufferBytes > 1048576) throw new ArgumentOutOfRangeException("readBufferBytes");

            this.stream = stream;
            this.encoding = encoding;
            this.maximumRawBytes = maximumRawBytes;
            this.maximumRows = maximumRows;
            this.maximumRowCharacters = maximumRowCharacters;
            this.maximumCanonicalRowBytes = maximumCanonicalRowBytes;
            this.maximumCanonicalBytes = maximumCanonicalBytes;
            this.readBufferBytes = readBufferBytes;
            this.rawRows = new List<string>();
        }

        public Task ReadTask { get; private set; }

        public long RawByteCount
        {
            get { return Interlocked.Read(ref rawByteCount); }
        }

        public int RowCount
        {
            get { return Volatile.Read(ref observedRowCount); }
        }

        public long ReadElapsedMilliseconds
        {
            get { return Interlocked.Read(ref readElapsedMilliseconds); }
        }

        public long NormalizationElapsedMilliseconds
        {
            get { return Interlocked.Read(ref normalizationElapsedMilliseconds); }
        }

        public long CanonicalHashElapsedMilliseconds
        {
            get { return Interlocked.Read(ref canonicalHashElapsedMilliseconds); }
        }

        public long CanonicalByteCount
        {
            get { return Interlocked.Read(ref canonicalByteCount); }
        }

        public string Sha256
        {
            get { return sha256; }
        }

        public Task BeginRead()
        {
            if (ReadTask != null) throw new InvalidOperationException("Registry digest read was already started.");
            ReadTask = Task.Factory.StartNew(
                ReadCore,
                CancellationToken.None,
                TaskCreationOptions.LongRunning,
                TaskScheduler.Default);
            return ReadTask;
        }

        private void ReadCore()
        {
            Stopwatch stopwatch = Stopwatch.StartNew();
            byte[] byteBuffer = new byte[readBufferBytes];
            char[] characterBuffer = new char[encoding.GetMaxCharCount(readBufferBytes)];
            Decoder decoder = encoding.GetDecoder();
            StringBuilder currentRow = new StringBuilder();
            bool firstCharacter = true;
            try
            {
                while (true)
                {
                    int read = stream.Read(byteBuffer, 0, byteBuffer.Length);
                    if (read < 0 || read > byteBuffer.Length)
                    {
                        throw new InvalidDataException("[REGISTRY_DIGEST_READ_INVALID] Native query returned an invalid byte count.");
                    }
                    if (read == 0) break;
                    long totalBytes = Interlocked.Add(ref rawByteCount, read);
                    if (totalBytes > maximumRawBytes)
                    {
                        throw new InvalidDataException("[REGISTRY_DIGEST_RAW_BYTE_CEILING] Native query exceeded the raw-byte ceiling.");
                    }
                    DecodeBytes(decoder, byteBuffer, read, characterBuffer, false, currentRow, ref firstCharacter);
                }

                DecodeBytes(decoder, byteBuffer, 0, characterBuffer, true, currentRow, ref firstCharacter);
                if (currentRow.Length != 0) AddRawRow(currentRow);
            }
            catch (DecoderFallbackException exception)
            {
                throw new InvalidDataException("[REGISTRY_DIGEST_DECODING_INVALID] Native query output is not valid in the explicit encoding.", exception);
            }
            finally
            {
                stopwatch.Stop();
                Interlocked.Exchange(ref readElapsedMilliseconds, stopwatch.ElapsedMilliseconds);
            }
        }

        private void DecodeBytes(
            Decoder decoder,
            byte[] byteBuffer,
            int byteCount,
            char[] characterBuffer,
            bool flush,
            StringBuilder currentRow,
            ref bool firstCharacter)
        {
            int byteIndex = 0;
            bool completed = false;
            while (!completed)
            {
                int bytesUsed;
                int charactersUsed;
                decoder.Convert(
                    byteBuffer,
                    byteIndex,
                    byteCount - byteIndex,
                    characterBuffer,
                    0,
                    characterBuffer.Length,
                    flush,
                    out bytesUsed,
                    out charactersUsed,
                    out completed);
                byteIndex += bytesUsed;
                for (int index = 0; index < charactersUsed; index++)
                {
                    char character = characterBuffer[index];
                    if (firstCharacter)
                    {
                        firstCharacter = false;
                        if (character == '\uFEFF') continue;
                    }
                    sawAnyCharacter = true;
                    endedWithLineFeed = character == '\n';
                    if (character == '\n')
                    {
                        if (currentRow.Length != 0 && currentRow[currentRow.Length - 1] == '\r')
                        {
                            currentRow.Length -= 1;
                        }
                        AddRawRow(currentRow);
                        currentRow.Length = 0;
                    }
                    else
                    {
                        currentRow.Append(character);
                        if (currentRow.Length > maximumRowCharacters)
                        {
                            throw new InvalidDataException("[REGISTRY_DIGEST_ROW_LENGTH_CEILING] Native query row exceeded the character ceiling.");
                        }
                    }
                }
                if (byteCount == 0 && charactersUsed == 0) break;
            }
        }

        private void AddRawRow(StringBuilder row)
        {
            if (rawRows.Count >= maximumRows)
            {
                throw new InvalidDataException("[REGISTRY_DIGEST_ROW_CEILING] Native query exceeded the row ceiling.");
            }
            rawRows.Add(row.ToString());
            Volatile.Write(ref observedRowCount, rawRows.Count);
        }

        public void AppendStderrText(string stderr)
        {
            EnsureReadCompleted();
            if (canonicalRows != null) throw new InvalidOperationException("Registry digest rows were already normalized.");
            if (String.IsNullOrEmpty(stderr)) return;
            if (!sawAnyCharacter || endedWithLineFeed) AddRawRow(new StringBuilder());
            StringBuilder currentRow = new StringBuilder();
            for (int index = 0; index < stderr.Length; index++)
            {
                char character = stderr[index];
                if (character == '\n')
                {
                    if (currentRow.Length != 0 && currentRow[currentRow.Length - 1] == '\r')
                    {
                        currentRow.Length -= 1;
                    }
                    AddRawRow(currentRow);
                    currentRow.Length = 0;
                }
                else
                {
                    currentRow.Append(character);
                    if (currentRow.Length > maximumRowCharacters)
                    {
                        throw new InvalidDataException("[REGISTRY_DIGEST_ROW_LENGTH_CEILING] Native query stderr row exceeded the character ceiling.");
                    }
                }
            }
            if (currentRow.Length != 0) AddRawRow(currentRow);
            sawAnyCharacter = sawAnyCharacter || stderr.Length != 0;
            endedWithLineFeed = stderr.Length != 0 && stderr[stderr.Length - 1] == '\n';
        }

        public void Normalize(long budgetMilliseconds)
        {
            EnsureReadCompleted();
            if (canonicalRows != null) throw new InvalidOperationException("Registry digest rows were already normalized.");
            Stopwatch stopwatch = Stopwatch.StartNew();
            string[] normalized = new string[rawRows.Count];
            long totalBytes = 0;
            for (int index = 0; index < rawRows.Count; index++)
            {
                CheckBudget(stopwatch, budgetMilliseconds, index);
                string canonical = QuoteJsonString(rawRows[index].TrimEnd());
                int rowBytes = encoding.GetByteCount(canonical);
                if (rowBytes > maximumCanonicalRowBytes)
                {
                    throw new InvalidDataException("[REGISTRY_DIGEST_ROW_LENGTH_CEILING] Canonical row exceeded the byte ceiling.");
                }
                totalBytes += rowBytes + 2L;
                if (totalBytes > maximumCanonicalBytes)
                {
                    throw new InvalidDataException("[REGISTRY_DIGEST_CANONICAL_BYTE_CEILING] Canonical rows exceeded the aggregate-byte ceiling.");
                }
                normalized[index] = canonical;
            }
            CheckBudget(stopwatch, budgetMilliseconds, rawRows.Count);
            canonicalRows = normalized;
            rawRows.Clear();
            Interlocked.Exchange(ref canonicalByteCount, totalBytes);
            stopwatch.Stop();
            Interlocked.Exchange(ref normalizationElapsedMilliseconds, stopwatch.ElapsedMilliseconds);
        }

        public string Hash(long budgetMilliseconds)
        {
            if (canonicalRows == null) throw new InvalidOperationException("Registry digest rows were not normalized.");
            if (sha256 != null) throw new InvalidOperationException("Registry digest was already finalized.");
            Stopwatch stopwatch = Stopwatch.StartNew();
            CheckBudget(stopwatch, budgetMilliseconds, 0);
            Array.Sort(canonicalRows, new BudgetedStringComparer(stopwatch, budgetMilliseconds));
            CheckBudget(stopwatch, budgetMilliseconds, 0);
            using (SHA256 hash = SHA256.Create())
            {
                byte[] newline = new byte[] { 10 };
                for (int index = 0; index < canonicalRows.Length; index++)
                {
                    CheckBudget(stopwatch, budgetMilliseconds, index);
                    if (index != 0) hash.TransformBlock(newline, 0, newline.Length, newline, 0);
                    byte[] rowBytes = encoding.GetBytes(canonicalRows[index]);
                    if (rowBytes.Length != 0) hash.TransformBlock(rowBytes, 0, rowBytes.Length, rowBytes, 0);
                }
                hash.TransformFinalBlock(new byte[0], 0, 0);
                sha256 = ToLowerHex(hash.Hash);
            }
            CheckBudget(stopwatch, budgetMilliseconds, canonicalRows.Length);
            stopwatch.Stop();
            Interlocked.Exchange(ref canonicalHashElapsedMilliseconds, stopwatch.ElapsedMilliseconds);
            return sha256;
        }

        private void EnsureReadCompleted()
        {
            if (ReadTask == null || !ReadTask.IsCompleted)
            {
                throw new InvalidOperationException("Registry digest read has not completed.");
            }
            ReadTask.GetAwaiter().GetResult();
        }

        private static void CheckBudget(Stopwatch stopwatch, long budgetMilliseconds, int itemCount)
        {
            if (budgetMilliseconds < 1 || stopwatch.ElapsedMilliseconds > budgetMilliseconds)
            {
                throw new TimeoutException("[REGISTRY_DIGEST_TIMEOUT] Registry digest substage exceeded its remaining phase budget; itemCount=" + itemCount.ToString(CultureInfo.InvariantCulture) + ".");
            }
        }

        private sealed class BudgetedStringComparer : IComparer<string>
        {
            private readonly Stopwatch stopwatch;
            private readonly long budgetMilliseconds;
            private int comparisonCount;

            public BudgetedStringComparer(Stopwatch stopwatch, long budgetMilliseconds)
            {
                this.stopwatch = stopwatch;
                this.budgetMilliseconds = budgetMilliseconds;
            }

            public int Compare(string left, string right)
            {
                comparisonCount++;
                if ((comparisonCount & 1023) == 0)
                {
                    CheckBudget(stopwatch, budgetMilliseconds, comparisonCount);
                }
                return StringComparer.CurrentCultureIgnoreCase.Compare(left, right);
            }
        }

        private static string QuoteJsonString(string value)
        {
            StringBuilder builder = new StringBuilder(value.Length + 2);
            builder.Append('"');
            for (int index = 0; index < value.Length; index++)
            {
                char character = value[index];
                switch (character)
                {
                    case '"': builder.Append("\\\""); break;
                    case '\\': builder.Append("\\\\"); break;
                    case '\b': builder.Append("\\b"); break;
                    case '\t': builder.Append("\\t"); break;
                    case '\n': builder.Append("\\n"); break;
                    case '\f': builder.Append("\\f"); break;
                    case '\r': builder.Append("\\r"); break;
                    default:
                        if (character < 0x20 || character == 0x85 || character == 0x2028 || character == 0x2029)
                        {
                            builder.Append("\\u");
                            builder.Append(((int)character).ToString("x4", CultureInfo.InvariantCulture));
                        }
                        else if (char.IsSurrogate(character))
                        {
                            if (char.IsHighSurrogate(character) && index + 1 < value.Length && char.IsLowSurrogate(value[index + 1]))
                            {
                                builder.Append(character);
                                builder.Append(value[++index]);
                            }
                            else
                            {
                                builder.Append('\uFFFD');
                            }
                        }
                        else
                        {
                            builder.Append(character);
                        }
                        break;
                }
            }
            builder.Append('"');
            return builder.ToString();
        }

        private static string ToLowerHex(byte[] bytes)
        {
            StringBuilder builder = new StringBuilder(bytes.Length * 2);
            for (int index = 0; index < bytes.Length; index++)
            {
                builder.Append(bytes[index].ToString("x2", CultureInfo.InvariantCulture));
            }
            return builder.ToString();
        }
    }

    public sealed class BoundedTextCapture
    {
        private readonly Stream stream;
        private readonly Encoding encoding;
        private readonly long maximumBytes;
        private readonly int readBufferBytes;
        private long rawByteCount;
        private string text;

        public BoundedTextCapture(Stream stream, Encoding encoding, long maximumBytes, int readBufferBytes)
        {
            if (stream == null) throw new ArgumentNullException("stream");
            if (encoding == null) throw new ArgumentNullException("encoding");
            if (maximumBytes < 1) throw new ArgumentOutOfRangeException("maximumBytes");
            if (readBufferBytes < 1 || readBufferBytes > 1048576) throw new ArgumentOutOfRangeException("readBufferBytes");
            this.stream = stream;
            this.encoding = encoding;
            this.maximumBytes = maximumBytes;
            this.readBufferBytes = readBufferBytes;
        }

        public Task ReadTask { get; private set; }

        public long RawByteCount
        {
            get { return Interlocked.Read(ref rawByteCount); }
        }

        public string Text
        {
            get
            {
                if (ReadTask == null || !ReadTask.IsCompleted) throw new InvalidOperationException("Bounded text read has not completed.");
                ReadTask.GetAwaiter().GetResult();
                return text;
            }
        }

        public Task BeginRead()
        {
            if (ReadTask != null) throw new InvalidOperationException("Bounded text read was already started.");
            ReadTask = Task.Factory.StartNew(
                ReadCore,
                CancellationToken.None,
                TaskCreationOptions.LongRunning,
                TaskScheduler.Default);
            return ReadTask;
        }

        private void ReadCore()
        {
            byte[] buffer = new byte[readBufferBytes];
            MemoryStream captured = new MemoryStream();
            try
            {
                while (true)
                {
                    int read = stream.Read(buffer, 0, buffer.Length);
                    if (read < 0 || read > buffer.Length)
                    {
                        throw new InvalidDataException("[REGISTRY_DIGEST_STDERR_INVALID] Native query stderr returned an invalid byte count.");
                    }
                    if (read == 0) break;
                    long total = Interlocked.Add(ref rawByteCount, read);
                    if (total > maximumBytes)
                    {
                        throw new InvalidDataException("[REGISTRY_DIGEST_STDERR_CEILING] Native query stderr exceeded the byte ceiling.");
                    }
                    captured.Write(buffer, 0, read);
                }
                text = encoding.GetString(captured.ToArray()).TrimStart('\uFEFF');
            }
            catch (DecoderFallbackException exception)
            {
                throw new InvalidDataException("[REGISTRY_DIGEST_DECODING_INVALID] Native query stderr is not valid in the explicit encoding.", exception);
            }
            finally
            {
                captured.Dispose();
            }
        }
    }
}
