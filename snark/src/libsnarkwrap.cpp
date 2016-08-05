#include <sodium.h>
#include <iostream>
#include <stdexcept>
#include <assert.h>
#include "common/default_types/r1cs_ppzksnark_pp.hpp"
#include "algebra/curves/public_params.hpp"
#include "relations/arithmetic_programs/qap/qap.hpp"
#include "reductions/r1cs_to_qap/r1cs_to_qap.hpp"
#include "relations/constraint_satisfaction_problems/r1cs/examples/r1cs_examples.hpp"

using namespace std;
using namespace libsnark;

typedef default_r1cs_ppzksnark_pp curve_pp;
typedef default_r1cs_ppzksnark_pp::G1_type curve_G1;
typedef default_r1cs_ppzksnark_pp::G2_type curve_G2;
typedef default_r1cs_ppzksnark_pp::GT_type curve_GT;
typedef default_r1cs_ppzksnark_pp::Fp_type curve_Fr;

extern "C" void libsnarkwrap_init() {
    libsnark::inhibit_profiling_info = true;
    libsnark::inhibit_profiling_counters = true;
    assert(sodium_init() != -1);
    curve_pp::init_public_params();

    // Rust wrappers assume these sizes
    assert(sizeof(curve_Fr) == 8 * (4));
    assert(sizeof(curve_G1) == 8 * (4 * 3));
    assert(sizeof(curve_G2) == 8 * (4 * 2 * 3));
    assert(sizeof(curve_GT) == 8 * (4 * 6 * 2));

    // Rust wrappers assume alignment.
    // This will trip up enabling ate-pairing until
    // the wrappers are changed.
    assert(alignof(curve_Fr) == alignof(uint64_t));
    assert(alignof(curve_G1) == alignof(uint64_t));
    assert(alignof(curve_G2) == alignof(uint64_t));
    assert(alignof(curve_GT) == alignof(uint64_t));
}

// Fr

extern "C" curve_Fr libsnarkwrap_Fr_random() {
    return curve_Fr::random_element();
}

extern "C" curve_Fr libsnarkwrap_Fr_zero() {
    return curve_Fr::zero();
}

extern "C" curve_Fr libsnarkwrap_Fr_one() {
    return curve_Fr::one();
}

extern "C" curve_Fr libsnarkwrap_Fr_from(const char *a) {
    return curve_Fr(a);
}

extern "C" curve_Fr libsnarkwrap_Fr_exp(const curve_Fr *a, uint32_t b) {
    return (*a) ^ b;
}

extern "C" curve_Fr libsnarkwrap_Fr_add(const curve_Fr *a, const curve_Fr *b) {
    return *a + *b;
}

extern "C" curve_Fr libsnarkwrap_Fr_sub(const curve_Fr *a, const curve_Fr *b) {
    return *a - *b;
}

extern "C" curve_Fr libsnarkwrap_Fr_mul(const curve_Fr *a, const curve_Fr *b) {
    return *a * *b;
}

extern "C" curve_Fr libsnarkwrap_Fr_neg(const curve_Fr *a) {
    return -(*a);
}

extern "C" curve_Fr libsnarkwrap_Fr_inverse(const curve_Fr *a) {
    return a->inverse();
}

extern "C" bool libsnarkwrap_Fr_is_zero(const curve_Fr *a) {
    return a->is_zero();
}

// G1

extern "C" curve_G1 libsnarkwrap_G1_zero() {
    return curve_G1::zero();
}

extern "C" curve_G1 libsnarkwrap_G1_one() {
    return curve_G1::one();
}

extern "C" curve_G1 libsnarkwrap_G1_random() {
    return curve_G1::random_element();
}

extern "C" bool libsnarkwrap_G1_is_zero(const curve_G1 *p) {
    return p->is_zero();
}

extern "C" bool libsnarkwrap_G1_is_equal(const curve_G1 *p, const curve_G1 *q) {
    return *p == *q;
}

extern "C" curve_G1 libsnarkwrap_G1_add(const curve_G1 *p, const curve_G1 *q) {
    return *p + *q;
}

extern "C" curve_G1 libsnarkwrap_G1_sub(const curve_G1 *p, const curve_G1 *q) {
    return *p - *q;
}

extern "C" curve_G1 libsnarkwrap_G1_neg(const curve_G1 *p) {
    return -(*p);
}

extern "C" curve_G1 libsnarkwrap_G1_scalarmul(const curve_G1 *p, const curve_Fr *q) {
    return (*q) * (*p);
}

// G2

extern "C" curve_G2 libsnarkwrap_G2_zero() {
    return curve_G2::zero();
}

extern "C" curve_G2 libsnarkwrap_G2_one() {
    return curve_G2::one();
}

extern "C" curve_G2 libsnarkwrap_G2_random() {
    return curve_G2::random_element();
}

extern "C" bool libsnarkwrap_G2_is_zero(const curve_G2 *p) {
    return p->is_zero();
}

extern "C" bool libsnarkwrap_G2_is_equal(const curve_G2 *p, const curve_G2 *q) {
    return *p == *q;
}

extern "C" curve_G2 libsnarkwrap_G2_add(const curve_G2 *p, const curve_G2 *q) {
    return *p + *q;
}

extern "C" curve_G2 libsnarkwrap_G2_sub(const curve_G2 *p, const curve_G2 *q) {
    return *p - *q;
}

extern "C" curve_G2 libsnarkwrap_G2_neg(const curve_G2 *p) {
    return -(*p);
}

extern "C" curve_G2 libsnarkwrap_G2_scalarmul(const curve_G2 *p, const curve_Fr *q) {
    return (*q) * (*p);
}

// Pairing

extern "C" curve_GT libsnarkwrap_gt_exp(const curve_GT *p, const curve_Fr *s) {
    return (*p) ^ (*s);
}

extern "C" curve_GT libsnarkwrap_pairing(const curve_G1 *p, const curve_G2 *q) {
    return curve_pp::reduced_pairing(*p, *q);
}

// QAP

qap_instance<curve_Fr> get_qap(
    std::shared_ptr<basic_radix2_domain<curve_Fr>> &domain
)
{
    // Generate a dummy circuit
    auto example = generate_r1cs_example_with_field_input<curve_Fr>(250, 4);

    // A/B swap
    example.constraint_system.swap_AB_if_beneficial();

    // QAP reduction
    auto qap = r1cs_to_qap_instance_map(example.constraint_system);

    // Degree of the QAP must be a power of 2
    assert(qap.degree() == 256);

    // Assume radix2 evaluation domain
    domain = std::static_pointer_cast<basic_radix2_domain<curve_Fr>>(qap.domain);

    return qap;
}

extern "C" void libsnarkwrap_getqap(uint32_t *d, curve_Fr *omega)
{
    std::shared_ptr<basic_radix2_domain<curve_Fr>> domain;
    auto qap = get_qap(domain);

    *omega = domain->omega;
    *d = qap.degree();
}

extern "C" bool libsnarkwrap_test_compare_tau(
    const curve_G1 *inputs,
    const curve_Fr *tau,
    uint32_t d
)
{
    std::shared_ptr<basic_radix2_domain<curve_Fr>> domain;
    auto qap = get_qap(domain);

    auto coeffs = domain->lagrange_coeffs(*tau);
    assert(coeffs.size() == d);
    assert(qap.degree() == d);

    bool res = true;
    for (size_t i = 0; i < d; i++) {
        res &= (coeffs[i] * curve_G1::one()) == inputs[i];
    }

    return res;
}