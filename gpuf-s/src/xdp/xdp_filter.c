#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/ip.h>
#include <linux/tcp.h>
#include <bpf/bpf_helpers.h>

#define IPPROTO_TCP 6

#define MAX_SCAN_LEN 256
#define TOKEN_LEN 16  // token length

struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 128);
    __type(key, __u8[TOKEN_LEN]);
    __type(value, __u8);
} api_keys SEC(".maps");


static __always_inline int match_prefix(void *data, void *data_end, const char *pat, int len) {
    if (data + len > data_end)
        return 0;
    return __builtin_memcmp(data, pat, len) == 0;
}

SEC("xdp")
int xdp_auth_filter(struct xdp_md *ctx) {
    void *data = (void *)(long)ctx->data;
    void *data_end = (void *)(long)ctx->data_end;

    struct ethhdr *eth = data;
    if ((void*)(eth + 1) > data_end)
        return XDP_PASS;
    
    __u16 h_proto = eth->h_proto;
    if (h_proto != __constant_htons(ETH_P_IP))
        return XDP_PASS;

    struct iphdr *iph = data + sizeof(*eth);
    if ((void*)(iph + 1) > data_end)
        return XDP_PASS;

    if (iph->protocol != IPPROTO_TCP)
        return XDP_PASS;
    
    if (iph->ihl < 5)
    return XDP_PASS;

    if ((void*)iph + iph->ihl * 4 > data_end)
        return XDP_PASS;


    struct tcphdr *tcph = (void*)iph + iph->ihl*4;
    if ((void*)(tcph + 1) > data_end)
        return XDP_PASS;
    
    if (tcph->doff < 5)
        return XDP_PASS;
    if ((void*)tcph + tcph->doff * 4 > data_end)
        return XDP_PASS;

    char *payload = (void*)tcph + tcph->doff*4;
    if (payload >= (char*)data_end)
        return XDP_PASS;

    int scan_len = MAX_SCAN_LEN;
    if (payload + scan_len > (char*)data_end)
        scan_len = (char*)data_end - payload;

    #pragma unroll
    for (int i = 0; i < MAX_SCAN_LEN; i++) {
        if (i + 14 + TOKEN_LEN > scan_len)
            break;
        if (match_prefix(payload + i, data_end, "Authorization:", 14)) {
            __u8 token[TOKEN_LEN];
            // #pragma unroll
            // for (int j = 0; j < TOKEN_LEN; j++) {
            //     token[j] = payload[i + 14 + j];
            // } 
            __builtin_memcpy(token, payload + i + 14, TOKEN_LEN);
            __u8 *val = bpf_map_lookup_elem(&api_keys, token);
            if (val)
                return XDP_PASS;
            else
                return XDP_DROP;
        }
    }

    return XDP_PASS;
}

char _license[] SEC("license") = "GPL";
