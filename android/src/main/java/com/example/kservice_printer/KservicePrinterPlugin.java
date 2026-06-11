package com.example.kservice_printer;

import android.content.Context;
import android.net.nsd.NsdManager;
import android.net.nsd.NsdServiceInfo;
import android.net.wifi.WifiManager;
import android.os.Handler;
import android.os.Looper;
import io.flutter.embedding.engine.plugins.FlutterPlugin;
import io.flutter.plugin.common.MethodCall;
import io.flutter.plugin.common.MethodChannel;
import io.flutter.plugin.common.MethodChannel.MethodCallHandler;
import io.flutter.plugin.common.MethodChannel.Result;
import java.net.InetAddress;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

public class KservicePrinterPlugin implements FlutterPlugin, MethodCallHandler {
    private static final String CHANNEL_NAME = "kservice_printer";

    private Context applicationContext;
    private Handler mainHandler;
    private MethodChannel channel;
    private NetworkDiscoveryRequest activeDiscoveryRequest;

    @Override
    public void onAttachedToEngine(FlutterPluginBinding binding) {
        applicationContext = binding.getApplicationContext();
        mainHandler = new Handler(Looper.getMainLooper());
        channel = new MethodChannel(binding.getBinaryMessenger(), CHANNEL_NAME);
        channel.setMethodCallHandler(this);
    }

    @Override
    public void onDetachedFromEngine(FlutterPluginBinding binding) {
        if (activeDiscoveryRequest != null) {
            activeDiscoveryRequest.cancel("Android 插件已销毁，网络扫描取消");
            activeDiscoveryRequest = null;
        }
        if (channel != null) {
            channel.setMethodCallHandler(null);
        }
        channel = null;
        mainHandler = null;
        applicationContext = null;
    }

    @Override
    public void onMethodCall(MethodCall call, Result result) {
        if (!"discoverNetworkPrinters".equals(call.method)) {
            result.notImplemented();
            return;
        }
        if (applicationContext == null || mainHandler == null) {
            result.success(errorResponse("Android 插件尚未完成初始化"));
            return;
        }
        if (activeDiscoveryRequest != null) {
            result.success(errorResponse("已有网络打印机扫描正在进行，请等待本次扫描结束"));
            return;
        }

        Number timeoutArg = call.argument("timeoutMs");
        long timeoutMs = timeoutArg == null ? 0L : timeoutArg.longValue();
        List<String> serviceTypes = new ArrayList<>();
        Object serviceTypesArg = call.argument("serviceTypes");
        if (serviceTypesArg instanceof List<?>) {
            for (Object item : (List<?>) serviceTypesArg) {
                if (item != null) {
                    serviceTypes.add(item.toString());
                }
            }
        }

        final NetworkDiscoveryRequest[] requestHolder = new NetworkDiscoveryRequest[1];
        requestHolder[0] =
                new NetworkDiscoveryRequest(
                        applicationContext,
                        mainHandler,
                        result,
                        timeoutMs,
                        serviceTypes,
                        () -> {
                            if (activeDiscoveryRequest == requestHolder[0]) {
                                activeDiscoveryRequest = null;
                            }
                        });
        activeDiscoveryRequest = requestHolder[0];
        requestHolder[0].start();
    }

    private static String errorResponse(String message) {
        try {
            JSONObject root = new JSONObject();
            root.put("ok", false);
            root.put("error", message);
            return root.toString();
        } catch (JSONException e) {
            return "{\"ok\":false,\"error\":\"网络打印机扫描失败\"}";
        }
    }

    @SuppressWarnings("deprecation")
    private static final class NetworkDiscoveryRequest {
        private static final long DEFAULT_DISCOVERY_TIMEOUT_MS = 3000L;
        private static final long MIN_DISCOVERY_TIMEOUT_MS = 250L;
        private static final long MAX_DISCOVERY_TIMEOUT_MS = 30000L;
        private static final String LOCAL_SUFFIX = ".local.";
        private static final String[] DEFAULT_SERVICE_TYPES = {
            "_pdl-datastream._tcp.local.",
            "_printer._tcp.local.",
            "_ipp._tcp.local.",
            "_ipps._tcp.local."
        };

        private final Context context;
        private final Handler mainHandler;
        private final Result result;
        private final Runnable onFinished;
        private final long timeoutMs;
        private final List<String> requestedServiceTypes;
        private final ArrayDeque<PendingResolve> resolveQueue = new ArrayDeque<>();
        private final Map<String, PrinterCandidate> candidates = new HashMap<>();
        private final Set<String> queuedServices = new HashSet<>();
        private final List<NsdManager.DiscoveryListener> discoveryListeners = new ArrayList<>();

        private NsdManager nsdManager;
        private WifiManager.MulticastLock multicastLock;
        private Runnable timeoutRunnable;
        private long startedAtMs;
        private boolean resolving;
        private boolean finished;

        NetworkDiscoveryRequest(
                Context context,
                Handler mainHandler,
                Result result,
                long timeoutMs,
                List<String> requestedServiceTypes,
                Runnable onFinished) {
            this.context = context.getApplicationContext();
            this.mainHandler = mainHandler;
            this.result = result;
            this.onFinished = onFinished;
            this.timeoutMs = normalizeTimeout(timeoutMs);
            this.requestedServiceTypes = requestedServiceTypes;
        }

        void start() {
            runOnMain(
                    () -> {
                        try {
                            startOnMain();
                        } catch (Exception e) {
                            finishError(messageFor(e));
                        }
                    });
        }

        void cancel(String message) {
            runOnMain(() -> finishError(message));
        }

        private void startOnMain() throws JSONException {
            nsdManager = (NsdManager) context.getSystemService(Context.NSD_SERVICE);
            if (nsdManager == null) {
                finishError("Android 系统未提供 NsdManager，无法扫描 mDNS 服务");
                return;
            }

            List<ServiceType> serviceTypes = normalizeServiceTypes(requestedServiceTypes);
            startedAtMs = nowMs();
            acquireMulticastLock();

            int startCount = 0;
            for (ServiceType serviceType : serviceTypes) {
                NsdManager.DiscoveryListener listener = discoveryListener(serviceType);
                try {
                    nsdManager.discoverServices(
                            serviceType.androidType,
                            NsdManager.PROTOCOL_DNS_SD,
                            listener);
                    discoveryListeners.add(listener);
                    startCount += 1;
                } catch (RuntimeException e) {
                    if (startCount == 0 && serviceType == serviceTypes.get(serviceTypes.size() - 1)) {
                        throw e;
                    }
                }
            }

            if (startCount == 0) {
                finishError("Android mDNS 服务发现启动失败");
                return;
            }

            timeoutRunnable = () -> finishSuccess(true);
            mainHandler.postDelayed(timeoutRunnable, timeoutMs);
        }

        private NsdManager.DiscoveryListener discoveryListener(ServiceType fallbackServiceType) {
            return new NsdManager.DiscoveryListener() {
                @Override
                public void onDiscoveryStarted(String serviceType) {}

                @Override
                public void onServiceFound(NsdServiceInfo serviceInfo) {
                    runOnMain(() -> enqueueResolve(serviceInfo, fallbackServiceType.publicType));
                }

                @Override
                public void onServiceLost(NsdServiceInfo serviceInfo) {}

                @Override
                public void onDiscoveryStopped(String serviceType) {}

                @Override
                public void onStartDiscoveryFailed(String serviceType, int errorCode) {
                    runOnMain(() -> handleStartDiscoveryFailed(this, errorCode));
                }

                @Override
                public void onStopDiscoveryFailed(String serviceType, int errorCode) {
                    runOnMain(() -> discoveryListeners.remove(this));
                }
            };
        }

        private void handleStartDiscoveryFailed(
                NsdManager.DiscoveryListener listener, int errorCode) {
            if (finished) {
                return;
            }
            tryStopDiscovery(listener);
            discoveryListeners.remove(listener);
            if (discoveryListeners.isEmpty()
                    && candidates.isEmpty()
                    && resolveQueue.isEmpty()
                    && !resolving) {
                finishError("Android mDNS 服务发现启动失败，错误码: " + errorCode);
            }
        }

        private void enqueueResolve(NsdServiceInfo serviceInfo, String fallbackServiceType) {
            if (finished) {
                return;
            }
            String serviceName = safeString(serviceInfo.getServiceName());
            String serviceType = normalizeServiceTypeForOutput(
                    serviceInfo.getServiceType(), fallbackServiceType);
            String key = serviceName + "|" + serviceType;
            if (!queuedServices.add(key)) {
                return;
            }
            resolveQueue.addLast(new PendingResolve(serviceInfo, serviceType));
            processNextResolve();
        }

        private void processNextResolve() {
            if (finished || resolving || resolveQueue.isEmpty()) {
                return;
            }
            PendingResolve pending = resolveQueue.removeFirst();
            resolving = true;
            try {
                nsdManager.resolveService(
                        pending.serviceInfo,
                        new NsdManager.ResolveListener() {
                            @Override
                            public void onResolveFailed(NsdServiceInfo serviceInfo, int errorCode) {
                                runOnMain(
                                        () -> {
                                            resolving = false;
                                            if (errorCode == NsdManager.FAILURE_ALREADY_ACTIVE
                                                    && pending.attempts < 2
                                                    && !finished) {
                                                pending.attempts += 1;
                                                resolveQueue.addFirst(pending);
                                                mainHandler.postDelayed(
                                                        NetworkDiscoveryRequest.this
                                                                ::processNextResolve,
                                                        100L);
                                            } else {
                                                processNextResolve();
                                            }
                                        });
                            }

                            @Override
                            public void onServiceResolved(NsdServiceInfo serviceInfo) {
                                runOnMain(
                                        () -> {
                                            addResolvedService(serviceInfo, pending.serviceType);
                                            resolving = false;
                                            processNextResolve();
                                        });
                            }
                        });
            } catch (RuntimeException e) {
                resolving = false;
                processNextResolve();
            }
        }

        private void addResolvedService(NsdServiceInfo serviceInfo, String fallbackServiceType) {
            if (finished) {
                return;
            }
            InetAddress hostAddress = serviceInfo.getHost();
            String host = hostAddress == null ? "" : safeString(hostAddress.getHostAddress());
            int port = serviceInfo.getPort();
            if (host.isEmpty() || port <= 0) {
                return;
            }

            String serviceType = normalizeServiceTypeForOutput(
                    serviceInfo.getServiceType(), fallbackServiceType);
            String serviceName = safeString(serviceInfo.getServiceName());
            String fullname = serviceName.isEmpty()
                    ? serviceType
                    : serviceName + "." + serviceType;
            List<String> addresses = new ArrayList<>();
            addresses.add(host);
            PrinterCandidate candidate =
                    new PrinterCandidate(
                            serviceName,
                            serviceType,
                            fullname,
                            host,
                            host,
                            port,
                            addresses,
                            txtAttributes(serviceInfo),
                            supportsRawTcp(serviceType, port));
            candidates.put(candidateKey(candidate), candidate);
        }

        private void finishSuccess(boolean timeoutReached) {
            if (finished) {
                return;
            }
            finished = true;
            if (timeoutRunnable != null) {
                mainHandler.removeCallbacks(timeoutRunnable);
            }
            stopDiscoveries();
            releaseMulticastLock();
            try {
                JSONObject root = new JSONObject();
                root.put("ok", true);
                root.put("result", resultJson(timeoutReached));
                onFinished.run();
                result.success(root.toString());
            } catch (JSONException e) {
                onFinished.run();
                result.success(errorResponse("网络打印机扫描结果编码失败: " + e.getMessage()));
            }
        }

        private void finishError(String message) {
            if (finished) {
                return;
            }
            finished = true;
            if (timeoutRunnable != null) {
                mainHandler.removeCallbacks(timeoutRunnable);
            }
            stopDiscoveries();
            releaseMulticastLock();
            onFinished.run();
            result.success(errorResponse(message));
        }

        private JSONObject resultJson(boolean timeoutReached) throws JSONException {
            long durationMs = Math.max(0L, nowMs() - startedAtMs);
            List<PrinterCandidate> printers = new ArrayList<>(candidates.values());
            Collections.sort(
                    printers,
                    Comparator.comparing(
                                    (PrinterCandidate item) ->
                                            item.serviceName.toLowerCase(Locale.ROOT))
                            .thenComparing(item -> item.host)
                            .thenComparingInt(item -> item.port));

            JSONArray printerArray = new JSONArray();
            for (PrinterCandidate printer : printers) {
                printerArray.put(printer.toJson());
            }

            JSONArray serviceTypeArray = new JSONArray();
            for (ServiceType serviceType : normalizeServiceTypes(requestedServiceTypes)) {
                serviceTypeArray.put(serviceType.publicType);
            }

            JSONObject payload = new JSONObject();
            payload.put("timeoutMs", timeoutMs);
            payload.put("durationMs", durationMs);
            payload.put("timedOut", timeoutReached && durationMs >= timeoutMs);
            payload.put("serviceTypes", serviceTypeArray);
            payload.put("printers", printerArray);
            return payload;
        }

        private void acquireMulticastLock() {
            WifiManager wifiManager =
                    (WifiManager) context.getApplicationContext().getSystemService(Context.WIFI_SERVICE);
            if (wifiManager == null) {
                return;
            }
            multicastLock = wifiManager.createMulticastLock("kservice_printer_mdns");
            multicastLock.setReferenceCounted(false);
            multicastLock.acquire();
        }

        private void releaseMulticastLock() {
            if (multicastLock == null) {
                return;
            }
            try {
                if (multicastLock.isHeld()) {
                    multicastLock.release();
                }
            } catch (RuntimeException ignored) {
            } finally {
                multicastLock = null;
            }
        }

        private void stopDiscoveries() {
            if (nsdManager == null) {
                discoveryListeners.clear();
                return;
            }
            for (NsdManager.DiscoveryListener listener : new ArrayList<>(discoveryListeners)) {
                tryStopDiscovery(listener);
            }
            discoveryListeners.clear();
            resolveQueue.clear();
        }

        private void tryStopDiscovery(NsdManager.DiscoveryListener listener) {
            if (nsdManager == null) {
                return;
            }
            try {
                nsdManager.stopServiceDiscovery(listener);
            } catch (RuntimeException ignored) {
            }
        }

        private static long normalizeTimeout(long timeoutMs) {
            long value = timeoutMs == 0L ? DEFAULT_DISCOVERY_TIMEOUT_MS : timeoutMs;
            return Math.max(MIN_DISCOVERY_TIMEOUT_MS, Math.min(MAX_DISCOVERY_TIMEOUT_MS, value));
        }

        private static List<ServiceType> normalizeServiceTypes(List<String> requestedTypes) {
            List<String> source = requestedTypes == null || requestedTypes.isEmpty()
                    ? defaultServiceTypes()
                    : requestedTypes;
            List<ServiceType> normalized = new ArrayList<>();
            Set<String> seen = new HashSet<>();
            for (String requestedType : source) {
                String publicType = normalizeMdnsServiceType(requestedType);
                if (seen.add(publicType)) {
                    normalized.add(new ServiceType(publicType, androidServiceType(publicType)));
                }
            }
            return normalized;
        }

        private static List<String> defaultServiceTypes() {
            List<String> values = new ArrayList<>();
            Collections.addAll(values, DEFAULT_SERVICE_TYPES);
            return values;
        }

        private static String normalizeMdnsServiceType(String serviceType) {
            String value = safeString(serviceType)
                    .trim()
                    .toLowerCase(Locale.ROOT);
            while (value.endsWith(".")) {
                value = value.substring(0, value.length() - 1);
            }
            if (value.isEmpty()) {
                throw new IllegalArgumentException("mDNS 服务类型不能为空");
            }
            if (value.endsWith(".local")) {
                value = value.substring(0, value.length() - ".local".length());
            }
            if (!value.startsWith("_")) {
                value = "_" + value;
            }
            if (!value.contains("._tcp") && !value.contains("._udp")) {
                if (value.contains(".")) {
                    throw new IllegalArgumentException(
                            "mDNS 服务类型缺少 _tcp 或 _udp 协议段: " + serviceType);
                }
                value = value + "._tcp";
            }
            value = value + LOCAL_SUFFIX;
            if (!value.endsWith("._tcp.local.") && !value.endsWith("._udp.local.")) {
                throw new IllegalArgumentException(
                        "mDNS 服务类型必须以 ._tcp.local. 或 ._udp.local. 结尾: "
                                + serviceType);
            }
            return value;
        }

        private static String normalizeServiceTypeForOutput(
                String serviceType, String fallbackServiceType) {
            try {
                return normalizeMdnsServiceType(serviceType);
            } catch (RuntimeException e) {
                return fallbackServiceType;
            }
        }

        private static String androidServiceType(String publicType) {
            String stripped = publicType.endsWith(LOCAL_SUFFIX)
                    ? publicType.substring(0, publicType.length() - LOCAL_SUFFIX.length())
                    : publicType;
            return stripped + ".";
        }

        private static Map<String, String> txtAttributes(NsdServiceInfo serviceInfo) {
            Map<String, String> txt = new HashMap<>();
            Map<String, byte[]> attributes = serviceInfo.getAttributes();
            if (attributes == null) {
                return txt;
            }
            for (Map.Entry<String, byte[]> entry : attributes.entrySet()) {
                byte[] value = entry.getValue();
                txt.put(
                        entry.getKey(),
                        value == null ? "" : new String(value, StandardCharsets.UTF_8));
            }
            return txt;
        }

        private static boolean supportsRawTcp(String serviceType, int port) {
            String value = serviceType.toLowerCase(Locale.ROOT);
            return value.contains("_pdl-datastream._tcp") || port == 9100;
        }

        private static String candidateKey(PrinterCandidate candidate) {
            return candidate.host.isEmpty()
                    ? candidate.fullname + ":" + candidate.port
                    : candidate.host + ":" + candidate.port;
        }

        private static String messageFor(Exception e) {
            String message = e.getMessage();
            return message == null || message.isEmpty()
                    ? e.getClass().getSimpleName()
                    : message;
        }

        private static String safeString(String value) {
            return value == null ? "" : value;
        }

        private static long nowMs() {
            return android.os.SystemClock.elapsedRealtime();
        }

        private void runOnMain(Runnable runnable) {
            if (Looper.myLooper() == Looper.getMainLooper()) {
                runnable.run();
            } else {
                mainHandler.post(runnable);
            }
        }
    }

    private static final class ServiceType {
        final String publicType;
        final String androidType;

        ServiceType(String publicType, String androidType) {
            this.publicType = publicType;
            this.androidType = androidType;
        }
    }

    private static final class PendingResolve {
        final NsdServiceInfo serviceInfo;
        final String serviceType;
        int attempts;

        PendingResolve(NsdServiceInfo serviceInfo, String serviceType) {
            this.serviceInfo = serviceInfo;
            this.serviceType = serviceType;
        }
    }

    private static final class PrinterCandidate {
        final String serviceName;
        final String serviceType;
        final String fullname;
        final String hostname;
        final String host;
        final int port;
        final List<String> addresses;
        final Map<String, String> txt;
        final boolean supportsRawTcp;

        PrinterCandidate(
                String serviceName,
                String serviceType,
                String fullname,
                String hostname,
                String host,
                int port,
                List<String> addresses,
                Map<String, String> txt,
                boolean supportsRawTcp) {
            this.serviceName = serviceName;
            this.serviceType = serviceType;
            this.fullname = fullname;
            this.hostname = hostname;
            this.host = host;
            this.port = port;
            this.addresses = addresses;
            this.txt = txt;
            this.supportsRawTcp = supportsRawTcp;
        }

        JSONObject toJson() throws JSONException {
            JSONObject json = new JSONObject();
            json.put("serviceName", serviceName);
            json.put("serviceType", serviceType);
            json.put("fullname", fullname);
            json.put("hostname", hostname);
            json.put("host", host);
            json.put("port", port);

            JSONArray addressArray = new JSONArray();
            for (String address : addresses) {
                addressArray.put(address);
            }
            json.put("addresses", addressArray);

            JSONObject txtJson = new JSONObject();
            for (Map.Entry<String, String> entry : txt.entrySet()) {
                txtJson.put(entry.getKey(), entry.getValue());
            }
            json.put("txt", txtJson);
            json.put("supportsRawTcp", supportsRawTcp);
            return json;
        }
    }
}
