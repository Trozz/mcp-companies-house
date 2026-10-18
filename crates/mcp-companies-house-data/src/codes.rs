//! Enumerated filter values accepted by the advanced company search, exactly as Companies House
//! spells them (see `company_status`, `company_type` and `company_subtype` in
//! <https://github.com/companieshouse/api-enumerations/blob/master/constants.yml>).

use rmcp::schemars::JsonSchema;
use serde::Deserialize;

macro_rules! code_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $code:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
        #[schemars(crate = "rmcp::schemars")]
        pub enum $name {
            $(#[serde(rename = $code)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $code),+
                }
            }
        }
    };
}

code_enum! {
    /// Company status, e.g. active, dissolved, liquidation.
    CompanyStatus {
        Active = "active",
        Dissolved = "dissolved",
        Open = "open",
        Closed = "closed",
        ConvertedClosed = "converted-closed",
        Receivership = "receivership",
        Administration = "administration",
        Liquidation = "liquidation",
        InsolvencyProceedings = "insolvency-proceedings",
        VoluntaryArrangement = "voluntary-arrangement",
        Registered = "registered",
        Removed = "removed",
    }
}

code_enum! {
    /// Company type, e.g. ltd (private limited company), plc, llp, limited-partnership.
    CompanyType {
        PrivateUnlimited = "private-unlimited",
        Ltd = "ltd",
        Plc = "plc",
        OldPublicCompany = "old-public-company",
        PrivateLimitedGuarantNscLimitedExemption = "private-limited-guarant-nsc-limited-exemption",
        LimitedPartnership = "limited-partnership",
        PrivateLimitedGuarantNsc = "private-limited-guarant-nsc",
        ConvertedOrClosed = "converted-or-closed",
        PrivateUnlimitedNsc = "private-unlimited-nsc",
        PrivateLimitedSharesSection30Exemption = "private-limited-shares-section-30-exemption",
        ProtectedCellCompany = "protected-cell-company",
        AssuranceCompany = "assurance-company",
        OverseaCompany = "oversea-company",
        Eeig = "eeig",
        IcvcSecurities = "icvc-securities",
        IcvcWarrant = "icvc-warrant",
        IcvcUmbrella = "icvc-umbrella",
        RegisteredSocietyNonJurisdictional = "registered-society-non-jurisdictional",
        IndustrialAndProvidentSociety = "industrial-and-provident-society",
        NorthernIreland = "northern-ireland",
        NorthernIrelandOther = "northern-ireland-other",
        RoyalCharter = "royal-charter",
        InvestmentCompanyWithVariableCapital = "investment-company-with-variable-capital",
        UnregisteredCompany = "unregistered-company",
        Llp = "llp",
        Other = "other",
        EuropeanPublicLimitedLiabilityCompanySe = "european-public-limited-liability-company-se",
        UkEstablishment = "uk-establishment",
        ScottishPartnership = "scottish-partnership",
        CharitableIncorporatedOrganisation = "charitable-incorporated-organisation",
        ScottishCharitableIncorporatedOrganisation = "scottish-charitable-incorporated-organisation",
        FurtherEducationOrSixthFormCollegeCorporation = "further-education-or-sixth-form-college-corporation",
        RegisteredOverseasEntity = "registered-overseas-entity",
    }
}

code_enum! {
    /// Company subtype.
    CompanySubtype {
        CommunityInterestCompany = "community-interest-company",
        PrivateFundLimitedPartnership = "private-fund-limited-partnership",
    }
}

/// Comma-separated codes, the form the advanced search takes for several values.
pub fn join<T: Copy>(values: &[T], as_str: fn(T) -> &'static str) -> String {
    values
        .iter()
        .map(|v| as_str(*v))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_through_serde() {
        for s in CompanyStatus::ALL {
            let parsed: CompanyStatus = serde_json::from_value(s.as_str().into()).unwrap();
            assert_eq!(parsed, *s);
        }
        for t in CompanyType::ALL {
            let parsed: CompanyType = serde_json::from_value(t.as_str().into()).unwrap();
            assert_eq!(parsed, *t);
        }
        for t in CompanySubtype::ALL {
            let parsed: CompanySubtype = serde_json::from_value(t.as_str().into()).unwrap();
            assert_eq!(parsed, *t);
        }
    }

    #[test]
    fn codes_with_digits_keep_their_spelling() {
        assert_eq!(
            CompanyType::PrivateLimitedSharesSection30Exemption.as_str(),
            "private-limited-shares-section-30-exemption"
        );
        let t: CompanyType =
            serde_json::from_value("private-limited-shares-section-30-exemption".into()).unwrap();
        assert_eq!(t, CompanyType::PrivateLimitedSharesSection30Exemption);
    }

    #[test]
    fn unknown_codes_are_rejected() {
        assert!(serde_json::from_value::<CompanyStatus>("bogus".into()).is_err());
        assert!(serde_json::from_value::<CompanyType>("Ltd".into()).is_err());
    }

    #[test]
    fn schema_lists_every_code() {
        let schema = rmcp::schemars::schema_for!(CompanyType);
        let text = serde_json::to_string(&schema).unwrap();
        for t in CompanyType::ALL {
            assert!(text.contains(&format!("\"{}\"", t.as_str())), "{text}");
        }
    }

    #[test]
    fn join_uses_commas() {
        assert_eq!(
            join(
                &[CompanyStatus::Active, CompanyStatus::Liquidation],
                CompanyStatus::as_str
            ),
            "active,liquidation"
        );
    }
}
