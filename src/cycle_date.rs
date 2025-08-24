use chrono::{Datelike, Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents a date in the 5-character cycle format: YYMWD
/// YY = Year cycle (00-99, each "year" is exactly 52 weeks = 364 days)
/// M = Month (0-C, representing 13 months of 4 weeks each)
/// W = Week within month (0-3)
/// D = Day within week (0-6, Sunday=0)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CycleDate {
    pub year_cycle: u8,  // 0-99
    pub month: u8,       // 0-12 (displayed as 0-C)
    pub week: u8,        // 0-3
    pub day: u8,         // 0-6
}

impl CycleDate {
    /// Create a new CycleDate
    pub fn new(year_cycle: u8, month: u8, week: u8, day: u8) -> Result<Self, String> {
        if year_cycle > 99 {
            return Err("Year cycle must be 0-99".to_string());
        }
        if month > 12 {
            return Err("Month must be 0-12".to_string());
        }
        if week > 3 {
            return Err("Week must be 0-3".to_string());
        }
        if day > 6 {
            return Err("Day must be 0-6".to_string());
        }
        
        Ok(CycleDate {
            year_cycle,
            month,
            week,
            day,
        })
    }
    
    /// Convert a real date to cycle date
    /// The epoch starts on the first Sunday of the system (you can adjust this)
    pub fn from_real_date(date: NaiveDate) -> Self {
        // Define epoch as January 1, 2024 (adjust as needed)
        let epoch = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        
        // Find the first Sunday on or after the epoch
        let days_to_sunday = (7 - epoch.weekday().num_days_from_sunday()) % 7;
        let cycle_start = epoch + Duration::days(days_to_sunday as i64);
        
        let days_since_start = (date - cycle_start).num_days();
        
        if days_since_start < 0 {
            // Handle dates before the cycle start
            return CycleDate::new(0, 0, 0, 0).unwrap();
        }
        
        let total_days = days_since_start as u64;
        
        // Each year is exactly 364 days (52 weeks)
        let year_cycle = ((total_days / 364) % 100) as u8;
        let days_in_year = total_days % 364;
        
        // Each month is exactly 28 days (4 weeks)
        let month = (days_in_year / 28) as u8;
        let days_in_month = days_in_year % 28;
        
        // Each week is 7 days
        let week = (days_in_month / 7) as u8;
        let day = (days_in_month % 7) as u8;
        
        CycleDate::new(year_cycle, month, week, day).unwrap()
    }
    
    /// Convert cycle date back to real date
    pub fn to_real_date(&self) -> NaiveDate {
        let epoch = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let days_to_sunday = (7 - epoch.weekday().num_days_from_sunday()) % 7;
        let cycle_start = epoch + Duration::days(days_to_sunday as i64);
        
        let total_days = (self.year_cycle as u64 * 364) +
                        (self.month as u64 * 28) +
                        (self.week as u64 * 7) +
                        self.day as u64;
        
        cycle_start + Duration::days(total_days as i64)
    }
    
    /// Get current cycle date
    pub fn today() -> Self {
        Self::from_real_date(Local::now().date_naive())
    }
    
    /// Format as 5-character string
    pub fn to_string(&self) -> String {
        let month_char = match self.month {
            0..=9 => (b'0' + self.month) as char,
            10 => 'A',
            11 => 'B',
            12 => 'C',
            _ => '?',
        };
        
        format!("{:02}{}{}{}", 
                self.year_cycle, 
                month_char, 
                self.week, 
                self.day)
    }
    
    /// Parse from 5-character string
    pub fn from_string(s: &str) -> Result<Self, String> {
        if s.len() != 5 {
            return Err("Cycle date must be exactly 5 characters".to_string());
        }
        
        let chars: Vec<char> = s.chars().collect();
        
        let year_cycle: u8 = format!("{}{}", chars[0], chars[1])
            .parse()
            .map_err(|_| "Invalid year cycle")?;
        
        let month = match chars[2] {
            '0'..='9' => chars[2] as u8 - b'0',
            'A' | 'a' => 10,
            'B' | 'b' => 11,
            'C' | 'c' => 12,
            _ => return Err("Invalid month character".to_string()),
        };
        
        let week: u8 = chars[3].to_digit(10)
            .ok_or("Invalid week")? as u8;
        
        let day: u8 = chars[4].to_digit(10)
            .ok_or("Invalid day")? as u8;
        
        Self::new(year_cycle, month, week, day)
    }
    
    /// Check if this is the first day of a week
    pub fn is_first_day_of_week(&self) -> bool {
        self.day == 0
    }
    
    /// Check if this is the first day of a month
    pub fn is_first_day_of_month(&self) -> bool {
        self.week == 0 && self.day == 0
    }
    
    /// Check if this is the first day of a year
    pub fn is_first_day_of_year(&self) -> bool {
        self.month == 0 && self.week == 0 && self.day == 0
    }
    
    /// Get the previous day
    pub fn previous_day(&self) -> Self {
        if self.day > 0 {
            return CycleDate::new(self.year_cycle, self.month, self.week, self.day - 1).unwrap();
        }
        
        if self.week > 0 {
            return CycleDate::new(self.year_cycle, self.month, self.week - 1, 6).unwrap();
        }
        
        if self.month > 0 {
            return CycleDate::new(self.year_cycle, self.month - 1, 3, 6).unwrap();
        }
        
        if self.year_cycle > 0 {
            return CycleDate::new(self.year_cycle - 1, 12, 3, 6).unwrap();
        }
        
        // Can't go before the first day of the first year
        CycleDate::new(0, 0, 0, 0).unwrap()
    }
    
    /// Get the next day
    pub fn next_day(&self) -> Self {
        if self.day < 6 {
            return CycleDate::new(self.year_cycle, self.month, self.week, self.day + 1).unwrap();
        }
        
        if self.week < 3 {
            return CycleDate::new(self.year_cycle, self.month, self.week + 1, 0).unwrap();
        }
        
        if self.month < 12 {
            return CycleDate::new(self.year_cycle, self.month + 1, 0, 0).unwrap();
        }
        
        if self.year_cycle < 99 {
            return CycleDate::new(self.year_cycle + 1, 0, 0, 0).unwrap();
        }
        
        // Wrap around after year 99
        CycleDate::new(0, 0, 0, 0).unwrap()
    }
    
    /// Get previous 7 days (including self)
    pub fn previous_week(&self) -> Vec<CycleDate> {
        let mut dates = Vec::new();
        let mut current = *self;
        
        for _ in 0..7 {
            dates.push(current);
            current = current.previous_day();
        }
        
        dates.reverse();
        dates
    }
}

impl fmt::Display for CycleDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cycle_date_creation() {
        let date = CycleDate::new(3, 11, 2, 5).unwrap();
        assert_eq!(date.to_string(), "03B25");
    }
    
    #[test]
    fn test_string_parsing() {
        let date = CycleDate::from_string("03B25").unwrap();
        assert_eq!(date.year_cycle, 3);
        assert_eq!(date.month, 11);
        assert_eq!(date.week, 2);
        assert_eq!(date.day, 5);
    }
    
    #[test]
    fn test_first_day_checks() {
        let date1 = CycleDate::new(1, 5, 2, 0).unwrap();
        assert!(date1.is_first_day_of_week());
        assert!(!date1.is_first_day_of_month());
        assert!(!date1.is_first_day_of_year());
        
        let date2 = CycleDate::new(1, 5, 0, 0).unwrap();
        assert!(date2.is_first_day_of_week());
        assert!(date2.is_first_day_of_month());
        assert!(!date2.is_first_day_of_year());
        
        let date3 = CycleDate::new(1, 0, 0, 0).unwrap();
        assert!(date3.is_first_day_of_week());
        assert!(date3.is_first_day_of_month());
        assert!(date3.is_first_day_of_year());
    }
    
    #[test]
    fn test_date_arithmetic() {
        let date = CycleDate::new(1, 5, 2, 3).unwrap();
        let next = date.next_day();
        let prev = next.previous_day();
        assert_eq!(date, prev);
    }
}
