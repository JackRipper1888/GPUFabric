#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/ip.h>
#include <linux/tcp.h>
#include <bpf/bpf_helpers.h>

#define IPPROTO_TCP 6

#define MAX_SCAN_LEN 256
#define TOKEN_LEN 16

struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 128);
    __type(key, __u8[TOKEN_LEN]);
    __type(value, __u8);
} api_keys SEC(".maps");

SEC("xdp")
int xdp_auth_filter(struct xdp_md *ctx) {
    void *data = (void *)(long)ctx->data;
    void *data_end = (void *)(long)ctx->data_end;

    /* Ethernet header */
    struct ethhdr *eth = data;
    if ((void *)(eth + 1) > data_end)
        return XDP_PASS;

    __u16 h_proto = eth->h_proto;
    if (h_proto != __constant_htons(ETH_P_IP))
        return XDP_PASS;

    /* IP header: make sure ihl valid and within packet */
    struct iphdr *iph = data + sizeof(*eth);
    if ((void *)(iph + 1) > data_end)
        return XDP_PASS;
    if (iph->protocol != IPPROTO_TCP)
        return XDP_PASS;
    if (iph->ihl < 5)
        return XDP_PASS;
    if ((void *)iph + iph->ihl * 4 > data_end)
        return XDP_PASS;

    /* TCP header: check doff and bounds */
    struct tcphdr *tcph = (void *)iph + iph->ihl * 4;
    if ((void *)(tcph + 1) > data_end)
        return XDP_PASS;
    if (tcph->doff < 5)
        return XDP_PASS;
    if ((void *)tcph + tcph->doff * 4 > data_end)
        return XDP_PASS;

    /* payload pointer */
    unsigned char *payload = (unsigned char *)tcph + tcph->doff * 4;
    if (payload >= (unsigned char *)data_end)
        return XDP_PASS;

    /* compute how many bytes we can safely scan */
    __u64 available = (__u64)((unsigned char *)data_end - payload);
    __u64 limit = available;
    if (limit > MAX_SCAN_LEN)
        limit = MAX_SCAN_LEN;

    /* prefix we search for */
    const char prefix[] = "Authorization:"; /* length 14 */
    const int prefix_len = 14;

    /* scan loop: *always* check pointer arithmetic before any read */
    #pragma unroll
    for (int i = 0; i < MAX_SCAN_LEN; i++) {
        /* ensure we don't run past our computed limit */
        if ((__u64)i + prefix_len + TOKEN_LEN > limit)
            break;

        /* explicit pointer check so verifier can see it */
        if (payload + i + prefix_len + TOKEN_LEN > (unsigned char *)data_end)
            break;

        /* now it's safe to compare prefix */
        if (__builtin_memcmp(payload + i, prefix, prefix_len) == 0) {
            /* safe to copy token (we already checked bounds) */
            __u8 token[TOKEN_LEN];
            __builtin_memcpy(token, payload + i + prefix_len, TOKEN_LEN);

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
